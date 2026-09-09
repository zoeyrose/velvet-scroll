//! Linux capture for selected mice, including composite gaming mouse nodes.
#[cfg(not(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
)))]
compile_error!("Velvet Scroll currently supports Linux x86_64 and aarch64; other input-event/ioctl ABIs require validation before capture is enabled.");
use crate::engine::{Engine, Settings};
use evdev::{raw_stream::RawDevice, AttributeSet, EventType, InputEvent, RelativeAxisCode};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::CString,
    fs, io,
    io::Write,
    os::fd::{AsRawFd, RawFd},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant, UNIX_EPOCH},
};

const PREFIX: &str = "Velvet Scroll";
const REL: u16 = 2;
const KEY: u16 = 1;
const WHEEL: u16 = 8;
const HWHEEL: u16 = 6;
const WHEEL_HI: u16 = 11;
const HWHEEL_HI: u16 = 12;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MouseInfo {
    pub id: String,
    pub name: String,
    pub selected: bool,
}

struct Candidate {
    info: MouseInfo,
    path: PathBuf,
}

// Sysfs is deliberately used for listing: selecting a mouse must also work before
// the administrator has installed its device ACL rule.
fn bits(text: &str) -> BTreeSet<u16> {
    text.split_whitespace()
        .rev()
        .enumerate()
        .flat_map(|(word, hex)| {
            let mask = u64::from_str_radix(hex, 16).unwrap_or(0);
            (0..usize::BITS).filter_map(move |bit| {
                ((mask & (1u64 << bit)) != 0)
                    .then_some((word * usize::BITS as usize + bit as usize) as u16)
            })
        })
        .collect()
}
fn read(base: &Path, file: &str) -> String {
    fs::read_to_string(base.join(file))
        .unwrap_or_default()
        .trim_end_matches('\n')
        .to_owned()
}
fn eligible(name: &str, events: &BTreeSet<u16>, keys: &BTreeSet<u16>, rel: &BTreeSet<u16>) -> bool {
    !name.starts_with(PREFIX)
        && keys.contains(&0x110)
        && !keys.contains(&0x145) // BTN_TOOL_FINGER: touchpad
        && !keys.contains(&0x14a) // BTN_TOUCH: touch/tablet
        && rel.contains(&0)
        && rel.contains(&1)
        && (rel.contains(&WHEEL) || rel.contains(&WHEEL_HI))
        && events.iter().all(|e| matches!(e, 0..=4 | 17 | 20))
}
fn supported_absolute_axes(axes: impl IntoIterator<Item = u16>) -> bool {
    !axes.into_iter().any(|code| (0x2f..=0x3d).contains(&code))
}
fn mirror_name(name: &str) -> io::Result<&str> {
    // Plasma keys per-device settings by vendor, product AND exact name.
    // Silently renaming/truncating a mirror resets the user's pointer profile.
    if name.len() >= 80 || name.contains('\0') {
        return Err(io::Error::other(
            "Mouse name cannot be mirrored exactly by uinput",
        ));
    }
    Ok(name)
}
fn is_mirror(phys: &str) -> bool {
    phys.starts_with("velvet-scroll/")
}
fn stable_id(path: &Path, base: &Path) -> io::Result<String> {
    let mut aliases = Vec::new();
    if let Ok(entries) = fs::read_dir("/dev/input/by-id") {
        for entry in entries.flatten() {
            if fs::canonicalize(entry.path()).ok().as_deref() == Some(path) {
                aliases.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
    }
    aliases.sort();
    if let Some(alias) = aliases.first() {
        return Ok(format!("by-id:{alias}"));
    }
    let phys = read(base, "phys");
    let uniq = read(base, "uniq");
    let identity = format!(
        "{}:{}:{}",
        read(base, "id/bustype"),
        read(base, "id/vendor"),
        read(base, "id/product")
    );
    if !phys.is_empty() {
        return Ok(format!("phys:{identity}:{phys}:{uniq}"));
    }
    if !uniq.is_empty() {
        return Ok(format!("uniq:{identity}:{uniq}"));
    }
    let sys = fs::canonicalize(base)?;
    // inputN/eventN are allocated anew after reconnect; retain the physical bus path.
    let components: Vec<_> = sys
        .components()
        .filter(|c| {
            let s = c.as_os_str().to_string_lossy();
            !["input", "event"].iter().any(|prefix| {
                s.strip_prefix(prefix)
                    .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
            })
        })
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    Ok(format!("sys:{identity}:{}", components.join("/")))
}
fn candidates() -> io::Result<Vec<Candidate>> {
    let entries = match fs::read_dir("/sys/class/input") {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e),
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let node = entry.file_name().to_string_lossy().into_owned();
        if !node
            .strip_prefix("event")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        {
            continue;
        }
        let base = entry.path().join("device");
        let name = read(&base, "name");
        if is_mirror(&read(&base, "phys")) {
            continue;
        }
        if !eligible(
            &name,
            &bits(&read(&base, "capabilities/ev")),
            &bits(&read(&base, "capabilities/key")),
            &bits(&read(&base, "capabilities/rel")),
        ) || !supported_absolute_axes(bits(&read(&base, "capabilities/abs")))
        {
            continue;
        }
        let path = PathBuf::from("/dev/input").join(node);
        if let Ok(id) = stable_id(&path, &base) {
            found.push(Candidate {
                info: MouseInfo {
                    id,
                    name,
                    selected: false,
                },
                path,
            });
        }
    }
    found.sort_by(|a, b| a.info.id.cmp(&b.info.id));
    // Ambiguous identities cannot safely be persisted as a selection.
    let duplicates: BTreeSet<_> = found
        .windows(2)
        .filter(|w| w[0].info.id == w[1].info.id)
        .map(|w| w[0].info.id.clone())
        .collect();
    found.retain(|c| !duplicates.contains(&c.info.id));
    Ok(found)
}
pub fn discover() -> io::Result<Vec<MouseInfo>> {
    Ok(candidates()?.into_iter().map(|c| c.info).collect())
}

struct Wheel {
    engine: Engine,
    high_resolution: bool,
    present: bool,
    remainder: i32,
}
impl Wheel {
    fn new(settings: Settings, high_resolution: bool, present: bool) -> Self {
        Self {
            engine: Engine::new(settings),
            high_resolution,
            present,
            remainder: 0,
        }
    }
    fn emit(&mut self, value: i32, horizontal: bool, out: &mut Vec<InputEvent>) {
        if value == 0 || !self.present {
            return;
        }
        let (low, high) = if horizontal {
            (HWHEEL, HWHEEL_HI)
        } else {
            (WHEEL, WHEEL_HI)
        };
        out.push(InputEvent::new(EventType::RELATIVE.0, high, value));
        let total = self.remainder as i64 + value as i64;
        let detents = total / 120;
        self.remainder = (total % 120) as i32;
        if detents != 0 {
            out.push(InputEvent::new(EventType::RELATIVE.0, low, detents as i32));
        }
    }
    fn cancel(&mut self) {
        self.engine.cancel();
    }
}

struct Frames {
    vertical: Wheel,
    horizontal: Wheel,
    held: BTreeSet<u16>,
    motion: i64,
    activity: bool,
    repeat: bool,
    absolute: BTreeMap<u16, i32>,
}
impl Frames {
    fn cancel(&mut self) {
        self.vertical.cancel();
        self.horizontal.cancel();
        self.motion = 0;
    }
    fn transform(&mut self, frame: &[InputEvent], enabled: bool, now: Duration) -> Vec<InputEvent> {
        let mut out = Vec::with_capacity(frame.len() + 4);
        let mut v: i32 = 0;
        let mut h: i32 = 0;
        let mut activity = false;
        for event in frame {
            match (event.event_type().0, event.code()) {
                (REL, WHEEL_HI) if self.vertical.high_resolution => {
                    v = v.saturating_add(event.value())
                }
                (REL, HWHEEL_HI) if self.horizontal.high_resolution => {
                    h = h.saturating_add(event.value())
                }
                (REL, WHEEL) if !self.vertical.high_resolution => {
                    v = v.saturating_add(event.value().saturating_mul(120))
                }
                (REL, HWHEEL) if !self.horizontal.high_resolution => {
                    h = h.saturating_add(event.value().saturating_mul(120))
                }
                (REL, WHEEL | HWHEEL | WHEEL_HI | HWHEEL_HI) => {}
                (KEY, _) if self.repeat && event.value() == 2 => {}
                (17 | 20, _) => {} // Output state is handled through uinput feedback.
                (3, code) => {
                    self.absolute.insert(code, event.value());
                    out.push(*event);
                }
                (KEY, code) => {
                    activity = true;
                    if event.value() == 0 {
                        self.held.remove(&code);
                    } else {
                        self.held.insert(code);
                    }
                    out.push(*event);
                }
                (REL, 0 | 1) => {
                    self.motion = self.motion.saturating_add(i64::from(event.value()).abs());
                    out.push(*event);
                }
                _ => out.push(*event),
            }
        }
        activity |= self.motion >= 2 || !self.held.is_empty();
        self.activity |= activity;
        if !enabled || activity {
            self.cancel();
        }
        if v != 0 || h != 0 {
            self.motion = 0;
        }
        if !enabled {
            // Preserve a physical hi-res device's legacy companion timing too.
            // Legacy-only input needs equivalent v120 events because our virtual
            // mouse advertises hi-res scrolling intentionally.
            out = frame
                .iter()
                .copied()
                .filter(|e| {
                    !(matches!(e.event_type().0, 17 | 20)
                        || self.repeat && e.event_type().0 == KEY && e.value() == 2)
                })
                .collect();
            if !self.vertical.high_resolution && v != 0 {
                out.push(InputEvent::new(REL, WHEEL_HI, v));
            }
            if !self.horizontal.high_resolution && h != 0 {
                out.push(InputEvent::new(REL, HWHEEL_HI, h));
            }
            self.vertical.remainder = 0;
            self.horizontal.remainder = 0;
            return out;
        }
        let v = self.vertical.engine.scroll(v, now);
        let h = self.horizontal.engine.scroll(h, now);
        // A click or movement in this frame may not arm a new coast.
        if activity {
            self.cancel();
        }
        self.vertical.emit(v, false, &mut out);
        self.horizontal.emit(h, true, &mut out);
        out
    }
}

pub struct Mouse {
    device: RawDevice,
    output: VirtualMouse,
    frames: Frames,
    pending: Vec<InputEvent>,
    dropped: bool,
    active: bool,
}

// evdev's builder omits LED/REP capabilities. These Linux UAPI layouts let a
// composite gaming mouse retain its keyboard keys, LEDs, and absolute controls.
#[repr(C)]
struct DeviceSetup {
    id: [u16; 4],
    name: [u8; 80],
    ff_effects_max: u32,
}
#[repr(C)]
struct AbsoluteSetup {
    code: u16,
    padding: u16,
    info: [i32; 6],
}
#[repr(C)]
#[derive(Clone, Copy)]
struct WireEvent {
    time: libc::timeval,
    kind: u16,
    code: u16,
    value: i32,
}
impl WireEvent {
    fn new(event: &InputEvent, time: Duration) -> Self {
        Self {
            time: libc::timeval {
                tv_sec: time.as_secs() as libc::time_t,
                tv_usec: time.subsec_micros() as libc::suseconds_t,
            },
            kind: event.event_type().0,
            code: event.code(),
            value: event.value(),
        }
    }
}
fn wire_frame(events: &[InputEvent], time: Duration) -> Vec<WireEvent> {
    events
        .iter()
        .chain(std::iter::once(&InputEvent::new(0, 0, 0)))
        .map(|event| WireEvent::new(event, time))
        .collect()
}
const fn ioctl_code(direction: u64, number: u8, size: usize) -> libc::c_ulong {
    ((direction << 30) | ((size as u64) << 16) | ((b'U' as u64) << 8) | number as u64)
        as libc::c_ulong
}
fn ioctl_value(fd: RawFd, number: u8, value: u16) -> io::Result<()> {
    // UI_SET_*BIT takes an integer value, despite the _IOW declaration.
    // SAFETY: fd is live and this command takes no userspace pointer.
    if unsafe { libc::ioctl(fd, ioctl_code(1, number, 4) as _, value as libc::c_int) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
struct VirtualMouse {
    file: fs::File,
}
impl VirtualMouse {
    fn create(
        device: &RawDevice,
        name: &str,
        phys: &CString,
        axes: &AttributeSet<RelativeAxisCode>,
    ) -> io::Result<Self> {
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open("/dev/uinput")?;
        let fd = file.as_raw_fd();
        for kind in device.supported_events().iter() {
            ioctl_value(fd, 100, kind.0)?;
        }
        for key in device.supported_keys().into_iter().flat_map(|s| s.iter()) {
            ioctl_value(fd, 101, key.0)?;
        }
        for axis in axes.iter() {
            ioctl_value(fd, 102, axis.0)?;
        }
        for code in device.misc_properties().into_iter().flat_map(|s| s.iter()) {
            ioctl_value(fd, 104, code.0)?;
        }
        for code in device.supported_leds().into_iter().flat_map(|s| s.iter()) {
            ioctl_value(fd, 105, code.0)?;
        }
        for prop in device.properties().iter() {
            ioctl_value(fd, 110, prop.0)?;
        }
        if device.supported_absolute_axes().is_some() {
            for (code, info) in device.get_absinfo()? {
                let setup = AbsoluteSetup {
                    code: code.0,
                    padding: 0,
                    info: [
                        info.value(),
                        info.minimum(),
                        info.maximum(),
                        info.fuzz(),
                        info.flat(),
                        info.resolution(),
                    ],
                };
                // SAFETY: setup matches uinput_abs_setup and lives through ioctl.
                if unsafe {
                    libc::ioctl(
                        fd,
                        ioctl_code(1, 4, std::mem::size_of::<AbsoluteSetup>()) as _,
                        &setup,
                    )
                } < 0
                {
                    return Err(io::Error::last_os_error());
                }
            }
        }
        // SAFETY: CString provides a live, NUL-terminated physical path.
        if unsafe {
            libc::ioctl(
                fd,
                ioctl_code(1, 108, std::mem::size_of::<*const libc::c_char>()) as _,
                phys.as_ptr(),
            )
        } < 0
        {
            return Err(io::Error::last_os_error());
        }
        let id = device.input_id();
        let mut setup = DeviceSetup {
            id: [id.bus_type().0, id.vendor(), id.product(), id.version()],
            name: [0; 80],
            ff_effects_max: 0,
        };
        let bytes = name.as_bytes();
        setup.name[..bytes.len()].copy_from_slice(bytes);
        // SAFETY: setup matches uinput_setup exactly, with an initialized name.
        if unsafe {
            libc::ioctl(
                fd,
                ioctl_code(1, 3, std::mem::size_of::<DeviceSetup>()) as _,
                &setup,
            )
        } < 0
        {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: UI_DEV_CREATE takes no argument; fd owns the configured device.
        if unsafe { libc::ioctl(fd, ioctl_code(0, 1, 0) as _) } < 0 {
            return Err(io::Error::last_os_error());
        }
        let mut output = Self { file };
        if let Some(repeat) = device.get_auto_repeat() {
            output.emit(&[
                InputEvent::new(20, 0, repeat.delay as i32),
                InputEvent::new(20, 1, repeat.period as i32),
            ])?;
        }
        if device.supported_leds().is_some() {
            let states = device.get_led_state()?;
            let events: Vec<_> = device
                .supported_leds()
                .into_iter()
                .flat_map(|s| s.iter())
                .map(|code| InputEvent::new(17, code.0, i32::from(states.contains(code))))
                .collect();
            output.emit(&events)?;
        }
        Ok(output)
    }
    fn get_syspath(&mut self) -> io::Result<PathBuf> {
        let mut name = [0u8; 128];
        // SAFETY: UI_GET_SYSNAME writes at most the size encoded in the command.
        if unsafe {
            libc::ioctl(
                self.file.as_raw_fd(),
                ioctl_code(2, 44, name.len()) as _,
                name.as_mut_ptr(),
            )
        } < 0
        {
            return Err(io::Error::last_os_error());
        }
        let len = name.iter().position(|&b| b == 0).unwrap_or(name.len());
        let name = std::str::from_utf8(&name[..len])
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        if !name
            .strip_prefix("input")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid uinput sysname",
            ));
        }
        Ok(PathBuf::from("/sys/devices/virtual/input").join(name))
    }
    fn emit(&mut self, events: &[InputEvent]) -> io::Result<()> {
        self.emit_at(events, monotonic_now()?)
    }
    fn emit_at(&mut self, events: &[InputEvent], time: Duration) -> io::Result<()> {
        // uinput accepts recent CLOCK_MONOTONIC timestamps. Keeping the source
        // SYN_REPORT time for every event preserves libinput velocity estimates
        // when several physical frames are drained together after a delay.
        let wire = wire_frame(events, time);
        // SAFETY: WireEvent is the C input_event layout, fully initialized, and
        // the temporary byte slice cannot outlive this contiguous vector.
        let bytes = unsafe {
            std::slice::from_raw_parts(
                wire.as_ptr().cast::<u8>(),
                wire.len() * std::mem::size_of::<WireEvent>(),
            )
        };
        self.file.write_all(bytes)
    }
    fn feedback(&mut self) -> io::Result<Vec<InputEvent>> {
        let zero = WireEvent::new(&InputEvent::new(0, 0, 0), Duration::ZERO);
        let mut events = [zero; 32];
        // SAFETY: read writes at most the array size to a valid initialized buffer.
        let count = unsafe {
            libc::read(
                self.file.as_raw_fd(),
                events.as_mut_ptr().cast(),
                std::mem::size_of_val(&events),
            )
        };
        if count < 0 {
            return Err(io::Error::last_os_error());
        }
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "virtual mouse disconnected",
            ));
        }
        if count as usize % std::mem::size_of::<WireEvent>() != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "partial uinput feedback event",
            ));
        }
        Ok(events[..count as usize / std::mem::size_of::<WireEvent>()]
            .iter()
            .map(|e| InputEvent::new(e.kind, e.code, e.value))
            .collect())
    }
}
impl Drop for VirtualMouse {
    fn drop(&mut self) {
        // SAFETY: destroying our own uinput device; fd close also destroys it.
        let _ = unsafe { libc::ioctl(self.file.as_raw_fd(), ioctl_code(0, 2, 0) as _) };
    }
}

fn nonblocking(device: &RawDevice) -> io::Result<()> {
    // SAFETY: the fd is owned by device and valid for both fcntl calls.
    let flags = unsafe { libc::fcntl(device.as_raw_fd(), libc::F_GETFL) };
    if flags < 0
        || unsafe { libc::fcntl(device.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
fn ready(output: &mut VirtualMouse) -> io::Result<()> {
    let sys = output.get_syspath()?;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(2) {
        for entry in fs::read_dir(&sys)?.flatten() {
            if entry.file_name().to_string_lossy().starts_with("event") {
                let node = PathBuf::from("/dev/input").join(entry.file_name());
                if let Ok(meta) = fs::metadata(&node) {
                    let dev = meta.rdev();
                    let initialized = fs::read_to_string(format!(
                        "/run/udev/data/c{}:{}",
                        libc::major(dev),
                        libc::minor(dev)
                    ))
                    .is_ok_and(|data| crate::metadata::initialized(&data));
                    if initialized {
                        // Kernel node/udev readiness precedes compositor discovery.
                        // Physical input is still ungrabbed throughout this grace period.
                        thread::sleep(Duration::from_millis(250));
                        return Ok(());
                    }
                }
            }
        }
        thread::sleep(Duration::from_millis(20));
    }
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        "Mouse calibration was not inherited. Install the matching Velvet Scroll package and udev rule, reload udev rules, then restart the service. Physical mouse was not grabbed",
    ))
}

pub fn prepare(id: &str, settings: Settings) -> io::Result<Mouse> {
    let candidate = candidates()?
        .into_iter()
        .find(|c| c.info.id == id)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "selected mouse is disconnected or unsupported",
            )
        })?;
    let device = RawDevice::open(&candidate.path)?;
    // Revalidate the open descriptor: hotplug may replace an event node after listing.
    let keys: BTreeSet<_> = device
        .supported_keys()
        .map(|s| s.iter().map(|x| x.0).collect())
        .unwrap_or_default();
    let rel: BTreeSet<_> = device
        .supported_relative_axes()
        .map(|s| s.iter().map(|x| x.0).collect())
        .unwrap_or_default();
    let events = device.supported_events().iter().map(|x| x.0).collect();
    let base = PathBuf::from("/sys/class/input")
        .join(
            candidate
                .path
                .file_name()
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid input path"))?,
        )
        .join("device");
    if is_mirror(device.physical_path().unwrap_or(""))
        || stable_id(&candidate.path, &base)? != id
        || device.physical_path().unwrap_or("") != read(&base, "phys")
        || device.unique_name().unwrap_or("") != read(&base, "uniq")
        || !supported_absolute_axes(
            device
                .supported_absolute_axes()
                .into_iter()
                .flat_map(|s| s.iter())
                .map(|c| c.0),
        )
        || !eligible(device.name().unwrap_or(""), &events, &keys, &rel)
        || device.name().unwrap_or("") != candidate.info.name
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "device changed or contains unsupported touch/force-feedback capabilities",
        ));
    }
    let vertical = rel.contains(&WHEEL) || rel.contains(&WHEEL_HI);
    let horizontal = rel.contains(&HWHEEL) || rel.contains(&HWHEEL_HI);
    let mut axes: AttributeSet<RelativeAxisCode> =
        rel.iter().map(|&x| RelativeAxisCode(x)).collect();
    // Legacy wheels intentionally gain hi-res output so fractional acceleration and
    // decaying coast work with libinput. Both protocols derive from one accumulator.
    if vertical {
        axes.insert(RelativeAxisCode(WHEEL));
        axes.insert(RelativeAxisCode(WHEEL_HI));
    }
    if horizontal {
        axes.insert(RelativeAxisCode(HWHEEL));
        axes.insert(RelativeAxisCode(HWHEEL_HI));
    }
    let name = mirror_name(&candidate.info.name)?;
    // A hidden marker excludes our mirrors without changing desktop identity.
    // The source event node also lets udev inherit effective DPI/wheel metadata.
    let source = candidate.path.file_name().unwrap().to_string_lossy();
    let phys = CString::new(format!("velvet-scroll/{source}")).map_err(io::Error::other)?;
    let flags = unsafe { libc::fcntl(device.as_raw_fd(), libc::F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    if (device.supported_leds().is_some() || device.get_auto_repeat().is_some())
        && flags & libc::O_ACCMODE != libc::O_RDWR
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "composite mouse requires read/write access to preserve LEDs and keyboard repeat",
        ));
    }
    let absolute = if device.supported_absolute_axes().is_some() {
        device
            .get_absinfo()?
            .map(|(c, i)| (c.0, i.value()))
            .collect()
    } else {
        BTreeMap::new()
    };
    let repeat = device.get_auto_repeat().is_some();
    let mut output = VirtualMouse::create(&device, name, &phys, &axes)?;
    ready(&mut output)?;
    nonblocking(&device)?;
    // EVIOCSCLOCKID: use the same clock for queued physical frames and coast ticks.
    let clock = libc::CLOCK_MONOTONIC;
    let request = (1u64 << 30) | (4 << 16) | ((b'E' as u64) << 8) | 0xa0;
    // SAFETY: ioctl receives a valid fd and pointer to an initialized clock id.
    if unsafe { libc::ioctl(device.as_raw_fd(), request as _, &clock) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(Mouse {
        device,
        output,
        frames: Frames {
            vertical: Wheel::new(settings, rel.contains(&WHEEL_HI), vertical),
            horizontal: Wheel::new(settings, rel.contains(&HWHEEL_HI), horizontal),
            held: BTreeSet::new(),
            motion: 0,
            activity: false,
            repeat,
            absolute,
        },
        pending: Vec::new(),
        dropped: false,
        active: false,
    })
}

fn button_changes(previous: &BTreeSet<u16>, current: &BTreeSet<u16>) -> Vec<InputEvent> {
    previous
        .difference(current)
        .map(|&k| InputEvent::new(KEY, k, 0))
        .chain(
            current
                .difference(previous)
                .map(|&k| InputEvent::new(KEY, k, 1)),
        )
        .collect()
}

fn monotonic_now() -> io::Result<Duration> {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: clock_gettime writes only the provided initialized timespec.
    if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut time) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(Duration::new(time.tv_sec as u64, time.tv_nsec as u32))
}

impl Mouse {
    /// Call on the event-loop thread after `prepare` finishes in a worker.
    pub fn activate(&mut self) -> io::Result<()> {
        if self.active {
            return Ok(());
        }
        if self.device.get_key_state()?.iter().next().is_some() {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "release buttons and keys on this mouse before enabling it",
            ));
        }
        self.device.grab()?;
        // Discard the pre-capture queue, then check state again under the grab.
        // A button pressed during handoff must remain on the physical device.
        // Bound draining so a busy mouse cannot starve the forwarding loop.
        let drain_result = (|| -> io::Result<()> {
            for _ in 0..32 {
                match self.device.fetch_events() {
                    Ok(events) => {
                        if events.count() == 0 {
                            return Ok(());
                        }
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(e),
                }
            }
            Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "mouse is too busy to activate; retrying",
            ))
        })();
        let state_result = self.device.get_key_state();
        if drain_result.is_err()
            || state_result
                .as_ref()
                .is_ok_and(|keys| keys.iter().next().is_some())
            || state_result.is_err()
        {
            self.device.ungrab()?;
            drain_result?;
            state_result?;
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "release buttons and keys on this mouse before enabling it",
            ));
        }
        self.active = true;
        Ok(())
    }
    pub fn has_held_buttons(&self) -> bool {
        !self.frames.held.is_empty()
    }
    pub fn take_activity(&mut self) -> bool {
        std::mem::take(&mut self.frames.activity)
    }
    pub fn output_fd(&self) -> RawFd {
        self.output.file.as_raw_fd()
    }
    pub fn process_feedback(&mut self) -> io::Result<()> {
        let result = self.feedback_inner();
        if result.is_err() {
            self.relinquish();
        }
        result
    }
    fn feedback_inner(&mut self) -> io::Result<()> {
        loop {
            let events = match self.output.feedback() {
                Ok(events) => events,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            };
            let mut forward: Vec<_> = events
                .into_iter()
                .filter(|e| matches!(e.event_type().0, 17 | 20))
                .collect();
            if !forward.is_empty() {
                forward.push(InputEvent::new(0, 0, 0));
                self.device.send_events(&forward)?;
            }
        }
    }
    pub fn fd(&self) -> RawFd {
        self.device.as_raw_fd()
    }
    pub fn cancel(&mut self) {
        self.frames.cancel();
    }
    pub fn set_settings(&mut self, settings: Settings) {
        self.cancel();
        self.frames.vertical.engine.set_settings(settings);
        self.frames.horizontal.engine.set_settings(settings);
    }
    pub fn is_coasting(&self) -> bool {
        self.frames.vertical.engine.is_coasting() || self.frames.horizontal.engine.is_coasting()
    }
    fn relinquish(&mut self) {
        self.cancel();
        let _ = self.release_buttons();
        let _ = self.device.ungrab();
        self.active = false;
    }
    fn release_buttons(&mut self) -> io::Result<()> {
        let events: Vec<_> = self
            .frames
            .held
            .iter()
            .map(|&k| InputEvent::new(EventType::KEY.0, k, 0))
            .collect();
        if !events.is_empty() {
            self.output.emit(&events)?;
        }
        self.frames.held.clear();
        Ok(())
    }
    fn recover(&mut self) -> io::Result<()> {
        self.cancel();
        self.pending.clear();
        let keys = self.device.get_key_state()?;
        let current: BTreeSet<_> = keys.iter().map(|k| k.0).collect();
        let mut events = button_changes(&self.frames.held, &current);
        if self.device.supported_absolute_axes().is_some() {
            for (code, info) in self.device.get_absinfo()? {
                if self.frames.absolute.get(&code.0) != Some(&info.value()) {
                    events.push(InputEvent::new(3, code.0, info.value()));
                    self.frames.absolute.insert(code.0, info.value());
                }
            }
        }
        if !events.is_empty() {
            self.output.emit(&events)?;
        }
        self.frames.held = current;
        Ok(())
    }
    pub fn process(&mut self, enabled: bool) -> io::Result<()> {
        if !self.active {
            return Ok(());
        }
        if !enabled {
            self.cancel();
        }
        let result = self.process_inner(enabled);
        if result.is_err() {
            self.relinquish();
        }
        result
    }
    fn process_inner(&mut self, enabled: bool) -> io::Result<()> {
        loop {
            let events: Vec<_> = match self.device.fetch_events() {
                Ok(events) => events.collect(),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            };
            if events.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "mouse disconnected",
                ));
            }
            for event in events {
                if event.event_type() == EventType::SYNCHRONIZATION {
                    if event.code() == 3 {
                        // SYN_DROPPED: ignore through next SYN_REPORT.
                        self.dropped = true;
                        self.frames.activity = true;
                        self.pending.clear();
                        self.cancel();
                    } else if event.code() == 0 {
                        if self.dropped {
                            self.recover()?;
                            self.dropped = false;
                        } else {
                            let now = match event.timestamp().duration_since(UNIX_EPOCH) {
                                Ok(now) => now,
                                Err(_) => monotonic_now()?,
                            };
                            let output = self.frames.transform(&self.pending, enabled, now);
                            self.pending.clear();
                            if !output.is_empty() {
                                self.output.emit_at(&output, now)?;
                            }
                        }
                    }
                } else if !self.dropped {
                    self.pending.push(event);
                    if self.pending.len() > 4096 {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "mouse sent an oversized event frame",
                        ));
                    }
                }
            }
        }
    }
    pub fn tick(&mut self) -> io::Result<()> {
        if !self.active {
            return Ok(());
        }
        let now = monotonic_now()?;
        let mut out = Vec::with_capacity(4);
        let v = self.frames.vertical.engine.tick(now);
        let h = self.frames.horizontal.engine.tick(now);
        self.frames.vertical.emit(v, false, &mut out);
        self.frames.horizontal.emit(h, true, &mut out);
        if out.is_empty() {
            return Ok(());
        }
        let result = self.output.emit(&out);
        if result.is_err() {
            self.relinquish();
        }
        result
    }
}
impl Drop for Mouse {
    fn drop(&mut self) {
        self.relinquish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn settings() -> Settings {
        Settings {
            acceleration: 0.0,
            coast: false,
        }
    }
    fn frames(hi: bool) -> Frames {
        Frames {
            vertical: Wheel::new(settings(), hi, true),
            horizontal: Wheel::new(settings(), false, true),
            held: BTreeSet::new(),
            motion: 0,
            activity: false,
            repeat: false,
            absolute: BTreeMap::new(),
        }
    }
    fn event(code: u16, value: i32) -> InputEvent {
        InputEvent::new(EventType::RELATIVE.0, code, value)
    }
    fn values(events: &[InputEvent], code: u16) -> i32 {
        events
            .iter()
            .filter(|e| e.event_type() == EventType::RELATIVE && e.code() == code)
            .map(|e| e.value())
            .sum()
    }
    #[test]
    fn mirror_identity_preserves_pointer_settings_without_self_capture() {
        assert_eq!(mirror_name("Logitech G Pro ").unwrap(), "Logitech G Pro ");
        assert_eq!(mirror_name("Mouse 🐭").unwrap(), "Mouse 🐭");
        assert!(mirror_name(&"x".repeat(80)).is_err());
        assert!(is_mirror("velvet-scroll/event4"));
        assert!(is_mirror("velvet-scroll/usb-legacy-marker"));
        assert!(!is_mirror("usb-0000:0e:00.0-5/input2"));
    }
    #[test]
    fn hi_res_and_legacy_companions_are_counted_once_even_across_frames() {
        let mut f = frames(true);
        let out = f.transform(
            &[event(WHEEL_HI, 120), event(WHEEL, 1)],
            true,
            Duration::ZERO,
        );
        assert_eq!(values(&out, WHEEL_HI), 120);
        assert_eq!(values(&out, WHEEL), 1);
        assert!(f
            .transform(&[event(WHEEL, 1)], true, Duration::ZERO)
            .is_empty());
    }
    #[test]
    fn fractional_wheel_remainders_and_reversal_are_coherent() {
        let mut f = frames(true);
        assert_eq!(
            values(
                &f.transform(&[event(WHEEL_HI, 90)], true, Duration::ZERO),
                WHEEL
            ),
            0
        );
        assert_eq!(
            values(
                &f.transform(&[event(WHEEL_HI, 60)], true, Duration::ZERO),
                WHEEL
            ),
            1
        );
        assert_eq!(
            values(
                &f.transform(&[event(WHEEL_HI, -150)], true, Duration::ZERO),
                WHEEL
            ),
            -1
        );
        assert_eq!(f.vertical.remainder, 0);
    }
    #[test]
    fn legacy_conversion_preserves_pointer_buttons_and_horizontal_scroll() {
        let mut f = frames(false);
        let button = InputEvent::new(EventType::KEY.0, 0x110, 1);
        let pointer = event(0, 7);
        let out = f.transform(
            &[pointer, button, event(WHEEL, -2), event(HWHEEL, 1)],
            false,
            Duration::ZERO,
        );
        assert_eq!(&out[..2], &[pointer, button]);
        assert_eq!(values(&out, WHEEL_HI), -240);
        assert_eq!(values(&out, WHEEL), -2);
        assert_eq!(values(&out, HWHEEL_HI), 120);
        assert!(f.held.contains(&0x110));
    }
    #[test]
    fn composite_mouse_is_supported_but_touchpad_and_standalone_keyboard_are_rejected() {
        let rel = [0, 1, WHEEL].into_iter().collect();
        let mut keys = [0x110].into_iter().collect();
        assert!(eligible(
            "Mouse",
            &[0, 1, 2, 4].into_iter().collect(),
            &keys,
            &rel
        ));
        keys.insert(30);
        assert!(eligible("Logitech G Pro", &bits("12001f"), &keys, &rel));
        assert!(!eligible(
            "Keyboard",
            &[0, 1, 17, 20].into_iter().collect(),
            &[30].into_iter().collect(),
            &BTreeSet::new()
        ));
        assert!(!eligible(
            "Touchpad",
            &[0, 1, 2, 3].into_iter().collect(),
            &[0x110, 0x145].into_iter().collect(),
            &rel
        ));
        assert!(!eligible(
            "Velvet Scroll: Mouse",
            &[0, 1, 2].into_iter().collect(),
            &[0x110].into_iter().collect(),
            &rel
        ));
    }
    #[test]
    fn disabled_high_resolution_frames_are_unchanged() {
        let mut f = frames(true);
        let frame = [event(WHEEL_HI, 60), event(WHEEL, 1), event(0, 1)];
        assert_eq!(f.transform(&frame, false, Duration::ZERO), frame);
    }
    #[test]
    fn dropped_sync_preserves_unchanged_buttons_and_repairs_changes() {
        let old = [0x110, 0x111].into_iter().collect();
        let new = [0x110, 0x112].into_iter().collect();
        assert_eq!(
            button_changes(&old, &new),
            vec![
                InputEvent::new(KEY, 0x111, 0),
                InputEvent::new(KEY, 0x112, 1)
            ]
        );
    }
    #[test]
    fn motion_and_buttons_cancel_pending_coast_and_report_activity() {
        let settings = Settings {
            acceleration: 3.0,
            coast: true,
        };
        let mut f = Frames {
            vertical: Wheel::new(settings, true, true),
            horizontal: Wheel::new(settings, false, true),
            held: BTreeSet::new(),
            motion: 0,
            activity: false,
            repeat: false,
            absolute: BTreeMap::new(),
        };
        for t in [0, 20, 40, 60, 80] {
            f.transform(&[event(WHEEL_HI, 120)], true, Duration::from_millis(t));
        }
        assert!(f.vertical.engine.is_coasting());
        f.transform(&[event(0, 2)], true, Duration::from_millis(81));
        assert!(!f.vertical.engine.is_coasting());
        assert!(f.activity);
        f.activity = false;
        f.transform(
            &[InputEvent::new(KEY, 0x110, 1), event(WHEEL_HI, 120)],
            true,
            Duration::from_millis(100),
        );
        assert!(!f.vertical.engine.is_coasting());
        assert!(f.activity);
    }
    #[test]
    fn composite_keys_absolute_axes_and_repeat_are_forwarded_without_double_repeat() {
        let mut f = frames(false);
        f.repeat = true;
        let press = InputEvent::new(KEY, 30, 1);
        let absolute = InputEvent::new(3, 0, 123);
        let repeat = InputEvent::new(KEY, 30, 2);
        let release = InputEvent::new(KEY, 30, 0);
        assert_eq!(
            f.transform(&[press, absolute, repeat], true, Duration::ZERO),
            vec![press, absolute]
        );
        assert!(f.held.contains(&30));
        assert_eq!(f.absolute.get(&0), Some(&123));
        assert_eq!(
            f.transform(&[release], false, Duration::ZERO),
            vec![release]
        );
        assert!(!f.held.contains(&30));
        f.repeat = false;
        assert_eq!(f.transform(&[repeat], false, Duration::ZERO), vec![repeat]);
    }
    #[test]
    fn physical_led_and_repeat_echoes_do_not_form_feedback_loop() {
        let mut f = frames(false);
        let feedback = [InputEvent::new(17, 0, 1), InputEvent::new(20, 0, 250)];
        assert!(f.transform(&feedback, true, Duration::ZERO).is_empty());
        assert!(f.transform(&feedback, false, Duration::ZERO).is_empty());
    }
    #[test]
    fn linux_uinput_abi_matches_system_headers() {
        assert_eq!(std::mem::size_of::<DeviceSetup>(), 92);
        assert_eq!(std::mem::size_of::<AbsoluteSetup>(), 28);
        assert_eq!(std::mem::offset_of!(AbsoluteSetup, info), 4);
        assert_eq!(
            std::mem::size_of::<WireEvent>(),
            std::mem::size_of::<libc::input_event>()
        );
        assert_eq!(ioctl_code(0, 1, 0), 0x5501);
        assert_eq!(
            ioctl_code(1, 3, std::mem::size_of::<DeviceSetup>()),
            0x405c5503
        );
        assert_eq!(
            ioctl_code(1, 4, std::mem::size_of::<AbsoluteSetup>()),
            0x401c5504
        );
        assert_eq!(ioctl_code(2, 44, 128), 0x8080552c);
    }
    #[test]
    fn composite_volume_axis_is_supported_but_multitouch_requires_slot_recovery() {
        assert!(supported_absolute_axes([0x20]));
        assert!(!supported_absolute_axes([0x20, 0x2f]));
        assert!(!supported_absolute_axes([0x35]));
    }
    #[test]
    fn forwarded_frame_keeps_source_time_through_generated_syn_report() {
        let source_time = Duration::new(42, 123_456_789);
        let events = [event(0, 7), event(1, -3), event(WHEEL_HI, 120)];
        let wire = wire_frame(&events, source_time);
        assert_eq!(wire.len(), events.len() + 1);
        for packet in &wire {
            assert_eq!(packet.time.tv_sec, 42);
            assert_eq!(packet.time.tv_usec, 123_456);
        }
        for (packet, source) in wire.iter().zip(events) {
            assert_eq!(packet.kind, source.event_type().0);
            assert_eq!(packet.code, source.code());
            assert_eq!(packet.value, source.value());
        }
        let sync = wire.last().unwrap();
        assert_eq!((sync.kind, sync.code, sync.value), (0, 0, 0));
        let next = wire_frame(&[event(0, 7)], source_time + Duration::from_millis(8));
        assert_eq!(next[0].time.tv_usec - wire[0].time.tv_usec, 8_000);
        assert_eq!(next.last().unwrap().time.tv_usec, next[0].time.tv_usec);
    }
    #[test]
    fn sysfs_bitmap_word_order() {
        assert_eq!(bits("103"), [0, 1, 8].into_iter().collect());
        assert!(bits("1 0").contains(&(usize::BITS as u16)));
    }
}
