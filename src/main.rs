mod config;
mod control;
mod daemon;
mod engine;
mod input;
mod metadata;

use control::Request;
use std::{
    env, io,
    path::PathBuf,
    process::{self, Command},
    time::Duration,
};
const HELP: &str = "Velvet Scroll — precise little scrolls, magical big flicks.

Usage: velvet-scroll <command>

  gui                         Open settings and tray controls
  daemon                      Run the input service in this session
  status [--json]             Show service state and any device errors
  devices [--json]            List mice and their stable selection IDs
  enable | disable | toggle   Change acceleration immediately
  set acceleration <1..8>     Set maximum scroll gain (default: 3)
  set coast <on|off>          Enable optional momentum (default: off)
  select <device-id> <on|off>  Choose which mouse to process
  stop                        Stop the service and release all mice
  demo                        Print a deterministic acceleration example
  --help | --version

Start the daemon, then open the GUI to select a mouse. No mice are captured
until selected. Bind 'velvet-scroll toggle' in your desktop shortcut settings.
";
fn boolean(value: &str) -> io::Result<bool> {
    match value {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err(io::Error::other("Expected 'on' or 'off'")),
    }
}
fn gui() -> io::Result<()> {
    let exe = env::current_exe()?;
    let mut candidates = vec![];
    if let Some(path) = env::var_os("VELVET_SCROLL_GUI") {
        candidates.push(PathBuf::from(path));
    }
    if let Some(dir) = exe.parent() {
        candidates.push(dir.join("../libexec/velvet-scroll/velvet_scroll.py"));
        candidates.push(dir.join("../../ui/velvet_scroll.py"));
    }
    candidates.push(PathBuf::from(
        "/usr/local/share/velvet-scroll/ui/velvet_scroll.py",
    ));
    candidates.push(PathBuf::from(
        "/usr/share/velvet-scroll/ui/velvet_scroll.py",
    ));
    let script=candidates.into_iter().find(|p|p.is_file()).ok_or_else(||io::Error::other("Desktop app not found. Install it with 'make install' or set VELVET_SCROLL_GUI to ui/velvet_scroll.py"))?;
    use std::os::unix::process::CommandExt;
    Err(Command::new("python3")
        .arg(script)
        .env("VELVET_SCROLL_BIN", exe)
        .exec())
}
fn demo() {
    println!("Default acceleration (3× cap), coast off; units are wheel detents.\n");
    for (label, interval) in [("Careful scroll", 300), ("Quick flick", 25)] {
        let mut engine = engine::Engine::new(engine::Settings {
            acceleration: 3.0,
            coast: false,
        });
        let values: Vec<String> = (0..12)
            .map(|i| {
                format!(
                    "{:.2}",
                    engine.scroll(120, Duration::from_millis(i * interval)) as f64 / 120.0
                )
            })
            .collect();
        println!("{label} ({interval} ms/notch): {}", values.join("  "));
    }
}
fn run(args: &[String]) -> io::Result<()> {
    if let [command, marker] = args {
        if command == "udev-properties" {
            print!("{}", metadata::inherit(marker)?);
            return Ok(());
        }
    }

    if args.is_empty() || args == ["--help"] || args == ["help"] {
        print!("{HELP}");
        return Ok(());
    }
    if args == ["--version"] {
        println!("velvet-scroll {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args == ["daemon"] {
        return daemon::run();
    }
    if args == ["gui"] {
        return gui();
    }
    if args == ["demo"] {
        demo();
        return Ok(());
    }
    let json = args.last().is_some_and(|a| a == "--json");
    let parts: Vec<&str> = args.iter().map(String::as_str).collect();
    if matches!(parts.as_slice(), ["devices"] | ["devices", "--json"]) {
        let status = control::send(&Request::Status).or_else(|_| control::offline_devices())?;
        if json {
            println!("{}", serde_json::to_string(&status)?);
        } else {
            for d in &status.devices {
                println!(
                    "{} {}\n    {}",
                    if d.selected { "[on]" } else { "[off]" },
                    d.name,
                    d.id
                );
            }
            if status.devices.is_empty() {
                println!("No supported mice found. Check mouse connections and input permissions.");
            }
        }
        return Ok(());
    }
    let request = match parts.as_slice() {
        ["status"] | ["status", "--json"] => Request::Status,
        ["enable"] => Request::Enable,
        ["disable"] => Request::Disable,
        ["toggle"] => Request::Toggle,
        ["stop"] => Request::Stop,
        ["set", "acceleration", value] => {
            let value: f64 = value
                .parse()
                .map_err(|_| io::Error::other("Acceleration must be a number from 1 to 8"))?;
            let request = Request::SetAcceleration { value };
            control::apply(&config::Config::default(), &request).map_err(io::Error::other)?;
            request
        }
        ["set", "coast", value] => Request::SetCoast {
            value: boolean(value)?,
        },
        ["select", id, value] => Request::Select {
            id: (*id).into(),
            value: boolean(value)?,
        },
        _ => {
            return Err(io::Error::other(
                "Unknown command or arguments. Run 'velvet-scroll --help'.",
            ))
        }
    };
    let status = control::send(&request)?;
    if json {
        println!("{}", serde_json::to_string(&status)?);
    } else if let Some(error) = &status.error {
        return Err(io::Error::other(error.clone()));
    } else {
        println!(
            "Velvet Scroll: {} · acceleration {:.1}× · coast {}",
            if status.enabled {
                "enabled"
            } else {
                "disabled"
            },
            status.acceleration,
            if status.coast { "on" } else { "off" }
        );
        for error in &status.errors {
            eprintln!("{error}");
        }
        if matches!(request, Request::Stop) {
            println!("Service stopped; physical mice released.");
        }
    }
    if status.error.is_some() {
        process::exit(1);
    }
    Ok(())
}
fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if let Err(error) = run(&args) {
        if args.last().is_some_and(|a| a == "--json") {
            println!(
                "{}",
                serde_json::json!({"running":false,"error":error.to_string()})
            );
        } else {
            eprintln!("velvet-scroll: {error}");
        }
        process::exit(1);
    }
}
