//! udev IMPORT helper: inherit the source mouse's effective calibration metadata.
//! No input events are read and no user configuration is loaded by this command.
use std::{
    fs, io,
    os::unix::fs::{FileTypeExt, MetadataExt},
    path::Path,
};

const PROPERTIES: &[&str] = &[
    "MOUSE_DPI",
    "MOUSE_WHEEL_CLICK_ANGLE",
    "MOUSE_WHEEL_CLICK_COUNT",
    "MOUSE_WHEEL_CLICK_ANGLE_HORIZONTAL",
    "MOUSE_WHEEL_CLICK_COUNT_HORIZONTAL",
    "ID_BUS",
    "ID_SEAT",
    "WL_SEAT",
];
fn source_node(marker: &str) -> io::Result<&str> {
    let node = marker
        .strip_prefix("velvet-scroll/")
        .ok_or_else(|| io::Error::other("Not a Velvet Scroll device"))?;
    let digits = node.strip_prefix("event").unwrap_or("");
    if digits.is_empty() || digits.len() > 10 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(io::Error::other("Invalid source event node"));
    }
    Ok(node)
}
fn properties(database: &str) -> String {
    let mut output = String::new();
    for line in database.lines() {
        let Some((key, value)) = line.strip_prefix("E:").and_then(|l| l.split_once('=')) else {
            continue;
        };
        if PROPERTIES.contains(&key) && !value.chars().any(char::is_control) {
            output.push_str(key);
            output.push('=');
            output.push_str(value);
            output.push('\n');
        }
    }
    output
}
pub fn inherit(marker: &str) -> io::Result<String> {
    let node = source_node(marker)?;
    let phys = fs::read_to_string(Path::new("/sys/class/input").join(node).join("device/phys"))?;
    if phys.starts_with("velvet-scroll/") {
        return Err(io::Error::other(
            "Cannot inherit metadata from another mirror",
        ));
    }
    let meta = fs::metadata(Path::new("/dev/input").join(node))?;
    if !meta.file_type().is_char_device() {
        return Err(io::Error::other("Source is not an input device"));
    }
    let database = fs::read_to_string(format!(
        "/run/udev/data/c{}:{}",
        libc::major(meta.rdev()),
        libc::minor(meta.rdev())
    ))?;
    Ok(properties(&database) + "VELVET_SCROLL_METADATA=1\n")
}
pub fn initialized(database: &str) -> bool {
    database
        .lines()
        .any(|line| line == "E:VELVET_SCROLL_METADATA=1")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capture_requires_successful_metadata_import() {
        assert!(!initialized("E:ID_INPUT_MOUSE=1\n"));
        assert!(!initialized("E:VELVET_SCROLL_METADATA=0\n"));
        assert!(initialized("E:VELVET_SCROLL_METADATA=1\n"));
    }
    #[test]
    fn only_allow_expected_source_nodes() {
        assert_eq!(source_node("velvet-scroll/event4").unwrap(), "event4");
        for bad in [
            "event4",
            "velvet-scroll/../etc/passwd",
            "velvet-scroll/event4/other",
            "velvet-scroll/event",
            "velvet-scroll/event4\n",
        ] {
            assert!(source_node(bad).is_err());
        }
    }
    #[test]
    fn inherit_effective_calibration_without_serials_or_device_paths() {
        let database="E:MOUSE_DPI=400@1000 *800@1000 1600@1000\nE:ID_BUS=usb\nE:ID_SERIAL=private\nE:DEVNAME=/dev/input/event4\nE:MOUSE_WHEEL_CLICK_ANGLE=15\nE:MOUSE_DPI=bad\rvalue\n";
        assert_eq!(
            properties(database),
            "MOUSE_DPI=400@1000 *800@1000 1600@1000\nID_BUS=usb\nMOUSE_WHEEL_CLICK_ANGLE=15\n"
        );
    }
}
