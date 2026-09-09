use crate::{
    config::{self, Config},
    control::{self, Request, Status},
    input::{self, Mouse, MouseInfo},
};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, RawFd},
        unix::{
            fs::OpenOptionsExt,
            net::{UnixListener, UnixStream},
        },
    },
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

static STOP: AtomicBool = AtomicBool::new(false);
extern "C" fn stop_signal(_: libc::c_int) {
    STOP.store(true, Ordering::Relaxed);
}
struct SocketCleanup(std::path::PathBuf);
impl Drop for SocketCleanup {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
struct Client {
    socket: UnixStream,
    input: Vec<u8>,
    output: Vec<u8>,
    sent: usize,
    started: Instant,
}
impl Client {
    fn new(socket: UnixStream) -> io::Result<Self> {
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket,
            input: Vec::new(),
            output: Vec::new(),
            sent: 0,
            started: Instant::now(),
        })
    }
    fn read_request(&mut self) -> io::Result<Option<Result<Request, String>>> {
        let mut buf = [0; 2048];
        loop {
            match self.socket.read(&mut buf) {
                Ok(0) => return Err(io::Error::from(io::ErrorKind::UnexpectedEof)),
                Ok(n) => {
                    self.input.extend_from_slice(&buf[..n]);
                    if self.input.len() > 8192 {
                        return Err(io::Error::other("Request too large"));
                    }
                    if let Some(end) = self.input.iter().position(|&b| b == b'\n') {
                        return Ok(Some(control::parse_request(&self.input[..end])));
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(None),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
    }
    fn respond(&mut self, status: &Status) {
        self.output = serde_json::to_vec(status).expect("status serializable");
        self.output.push(b'\n');
    }
    fn flush(&mut self) -> io::Result<bool> {
        while self.sent < self.output.len() {
            match self.socket.write(&self.output[self.sent..]) {
                Ok(0) => return Err(io::Error::from(io::ErrorKind::WriteZero)),
                Ok(n) => self.sent += n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(false),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        Ok(!self.output.is_empty())
    }
}
struct State {
    config: Config,
    mice: HashMap<String, Mouse>,
    devices: Vec<MouseInfo>,
    errors: Vec<String>,
    preparing: HashSet<String>,
    sender: mpsc::Sender<(String, io::Result<Mouse>)>,
    receiver: mpsc::Receiver<(String, io::Result<Mouse>)>,
}
impl State {
    fn status(&self) -> Status {
        let mut status = Status::new(&self.config, self.devices.clone(), self.errors.clone());
        status.active_devices = self.mice.keys().cloned().collect();
        status.active_devices.sort();
        status.preparing_devices = self.preparing.iter().cloned().collect();
        status.preparing_devices.sort();
        status
    }
    fn reconcile(&mut self) {
        self.errors.clear();
        match input::discover() {
            Ok(devices) => self.devices = devices,
            Err(e) => {
                self.devices.clear();
                self.errors.push(format!("Cannot discover mice: {e}"));
            }
        }
        self.mice.retain(|id, _| {
            self.config.selected_devices.contains(id) && self.devices.iter().any(|d| &d.id == id)
        });
        for id in &self.config.selected_devices {
            if self.mice.contains_key(id) || self.preparing.contains(id) {
                continue;
            }
            if !self.devices.iter().any(|d| &d.id == id) {
                self.errors
                    .push(format!("Selected mouse is disconnected: {id}"));
                continue;
            }
            // Device discovery/udev readiness can take seconds. Keep existing mice flowing.
            if self.preparing.len() >= 4 {
                continue;
            }
            self.preparing.insert(id.clone());
            let sender = self.sender.clone();
            let id = id.clone();
            let settings = self.config.settings();
            std::thread::spawn(move || {
                let result = input::prepare(&id, settings);
                let _ = sender.send((id, result));
            });
        }
    }
    fn finish_preparing(&mut self) {
        while let Ok((id, result)) = self.receiver.try_recv() {
            self.preparing.remove(&id);
            if !self.config.selected_devices.contains(&id) || self.mice.contains_key(&id) {
                continue;
            }
            match result.and_then(|mut mouse| {
                mouse.set_settings(self.config.settings());
                mouse.activate()?;
                Ok(mouse)
            }) {
                Ok(mouse)=>{self.mice.insert(id,mouse);},
                Err(e)=>self.errors.push(format!("Cannot activate {id}: {e}. Check device permissions and close other input remappers.")),
            }
        }
    }
    fn handle(&mut self, request: Result<Request, String>) -> Status {
        let result = (|| {
            let request = request?;
            if matches!(request, Request::Stop) {
                STOP.store(true, Ordering::Relaxed);
                return Ok(());
            }
            if matches!(request, Request::Status) {
                return Ok(());
            }
            let next = control::apply(&self.config, &request)?;
            if next != self.config {
                config::save(&next).map_err(|e| format!("Could not save settings: {e}"))?;
                let selection_changed = next.selected_devices != self.config.selected_devices;
                self.config = next;
                for mouse in self.mice.values_mut() {
                    mouse.set_settings(self.config.settings());
                    mouse.cancel();
                }
                if selection_changed {
                    self.reconcile();
                }
            }
            Ok::<(), String>(())
        })();
        let mut status = self.status();
        if let Err(e) = result {
            status.error = Some(e);
        }
        status
    }
}
fn pollfd(fd: RawFd, events: i16) -> libc::pollfd {
    libc::pollfd {
        fd,
        events,
        revents: 0,
    }
}
fn boottime() -> Duration {
    let mut t = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe {
        libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut t);
    }
    Duration::new(t.tv_sec.max(0) as u64, t.tv_nsec.max(0) as u32)
}
pub fn run() -> io::Result<()> {
    STOP.store(false, Ordering::Relaxed);
    let dir = config::runtime_dir()?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(dir.join("daemon.lock"))?;
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(io::Error::other("Velvet Scroll is already running"));
    }
    let socket = dir.join("control.sock");
    match fs::remove_file(&socket) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let listener = UnixListener::bind(&socket)?;
    let _cleanup = SocketCleanup(socket);
    listener.set_nonblocking(true)?;
    unsafe {
        libc::signal(
            libc::SIGTERM,
            stop_signal as *const () as libc::sighandler_t,
        );
        libc::signal(libc::SIGINT, stop_signal as *const () as libc::sighandler_t);
    }
    let (sender, receiver) = mpsc::channel();
    let mut state = State {
        config: config::load()?,
        mice: HashMap::new(),
        devices: Vec::new(),
        errors: Vec::new(),
        preparing: HashSet::new(),
        sender,
        receiver,
    };
    state.reconcile();
    eprintln!(
        "Velvet Scroll ready. {} mouse/mice selected. Use 'velvet-scroll gui' for controls.",
        state.config.selected_devices.len()
    );
    for error in &state.errors {
        eprintln!("{error}");
    }
    let mut scan = Instant::now();
    let mut last_boot = boottime();
    let mut clients: Vec<Client> = vec![];
    while !STOP.load(Ordering::Relaxed) {
        state.finish_preparing();
        let ids: Vec<String> = state.mice.keys().cloned().collect();
        let mut fds = vec![pollfd(listener.as_raw_fd(), libc::POLLIN)];
        for id in &ids {
            fds.push(pollfd(state.mice[id].fd(), libc::POLLIN));
        }
        for id in &ids {
            fds.push(pollfd(state.mice[id].output_fd(), libc::POLLIN));
        }
        for c in &clients {
            fds.push(pollfd(
                c.socket.as_raw_fd(),
                if c.output.is_empty() {
                    libc::POLLIN
                } else {
                    libc::POLLOUT
                },
            ));
        }
        let coast = state.config.enabled && state.mice.values().any(Mouse::is_coasting);
        let scan_wait = Duration::from_secs(2).saturating_sub(scan.elapsed());
        let wait = if coast {
            scan_wait.min(Duration::from_millis(8))
        } else {
            scan_wait
        };
        let wait = if clients.is_empty() {
            wait
        } else {
            wait.min(Duration::from_millis(100))
        };
        let wait = if state.preparing.is_empty() {
            wait
        } else {
            wait.min(Duration::from_millis(10))
        };
        let result = unsafe {
            libc::poll(
                fds.as_mut_ptr(),
                fds.len() as libc::nfds_t,
                wait.as_millis() as i32,
            )
        };
        if result < 0 {
            let e = io::Error::last_os_error();
            if e.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(e);
        }
        let boot = boottime();
        // A long scheduling pause or suspend must never restart stale momentum.
        if boot.saturating_sub(last_boot) > Duration::from_millis(250) {
            for m in state.mice.values_mut() {
                m.cancel();
            }
        }
        last_boot = boot;
        let mut failed = vec![];
        let mut activity = false;
        for (i, id) in ids.iter().enumerate() {
            let mouse = state.mice.get_mut(id).unwrap();
            let revents = fds[i + 1].revents;
            let result = if revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                Err(io::Error::other("Mouse disconnected"))
            } else if revents & libc::POLLIN != 0 {
                mouse.process(state.config.enabled)
            } else {
                Ok(())
            };
            let feedback = fds[1 + ids.len() + i].revents;
            let result = result.and_then(|()| {
                if feedback & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                    Err(io::Error::other("Virtual mouse disconnected"))
                } else if feedback & libc::POLLIN != 0 {
                    mouse.process_feedback()
                } else {
                    Ok(())
                }
            });
            activity |= mouse.take_activity();
            if let Err(e) = result {
                state.errors.push(format!("Input released for {id}: {e}"));
                failed.push(id.clone());
            }
        }
        for id in failed {
            state.mice.remove(&id);
        }
        if activity || state.mice.values().any(Mouse::has_held_buttons) {
            for mouse in state.mice.values_mut() {
                mouse.cancel();
            }
        }
        if state.config.enabled {
            let failed: Vec<_> = state
                .mice
                .iter_mut()
                .filter_map(|(id, mouse)| {
                    if mouse.is_coasting() {
                        mouse.tick().err().map(|e| (id.clone(), e))
                    } else {
                        None
                    }
                })
                .collect();
            for (id, e) in failed {
                state.mice.remove(&id);
                state.errors.push(format!("Input released for {id}: {e}"));
            }
        }
        // Connections are nonblocking and bounded: a stalled UI never stalls pointer forwarding.
        let mut keep = Vec::with_capacity(clients.len());
        for mut client in clients.drain(..) {
            if client.started.elapsed() > Duration::from_secs(2) {
                continue;
            }
            if client.output.is_empty() {
                match client.read_request() {
                    Ok(Some(request)) => client.respond(&state.handle(request)),
                    Ok(None) => {}
                    Err(_) => continue,
                }
            }
            match client.flush() {
                Ok(true) | Err(_) => {}
                Ok(false) => keep.push(client),
            }
        }
        clients = keep;
        if fds[0].revents & libc::POLLIN != 0 {
            // Limit work per iteration even if a client floods connect().
            for _ in 0..16 {
                match listener.accept() {
                    Ok((socket, _)) => {
                        if clients.len() >= 16 {
                            continue;
                        }
                        let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
                        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
                        if unsafe {
                            libc::getsockopt(
                                socket.as_raw_fd(),
                                libc::SOL_SOCKET,
                                libc::SO_PEERCRED,
                                &mut cred as *mut _ as *mut _,
                                &mut len,
                            )
                        } == 0
                            && cred.uid == unsafe { libc::geteuid() }
                        {
                            if let Ok(client) = Client::new(socket) {
                                clients.push(client);
                            }
                        }
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(e),
                }
            }
        }
        if scan.elapsed() >= Duration::from_secs(2) {
            state.reconcile();
            scan = Instant::now();
        }
    }
    // Dropping mirrors closes exclusive grabs, restoring the physical devices.
    drop(state);
    drop(_cleanup);
    drop(lock);
    Ok(())
}
