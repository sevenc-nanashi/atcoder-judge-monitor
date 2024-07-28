use std::io::IsTerminal;
use std::sync::OnceLock;

static DEBUG: OnceLock<bool> = OnceLock::new();
static ANSI: OnceLock<bool> = OnceLock::new();

#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {
            $crate::log::_debug(format!($($arg)*))
    };
}

#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {
        $crate::log::_info(format!($($arg)*))
    };
}

#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => {
        $crate::log::_error(format!($($arg)*))
    };
}

#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {
        $crate::log::_warn(format!($($arg)*))
    };
}

#[macro_export]
macro_rules! question {
    ($($arg:tt)*) => {
        $crate::log::_question(format!($($arg)*))
    };
}

pub fn init() {
    let ansi = if std::env::var("NO_COLOR").is_ok() {
        false
    } else if std::env::var("FORCE_COLOR").is_ok() {
        true
    } else {
        std::io::stdout().is_terminal()
    };

    ANSI.set(ansi).unwrap();

    let debug = cfg!(debug_assertions) || std::env::var("DEBUG").is_ok();
    DEBUG.set(debug).unwrap();
}

pub fn strip_ansi_codes(input: &str) -> String {
    if *ANSI.get().unwrap() {
        return input.to_string();
    }
    return console::strip_ansi_codes(input).to_string();
}

pub fn _debug(message: String) {
    let debug = *DEBUG.get().unwrap();
    if debug {
        let ansi = *ANSI.get().unwrap();
        if ansi {
            println!("\x1b[1;90mD) \x1b[0m{}", message);
        } else {
            println!("D) {}", message);
        }
    }
}

pub fn _info(message: String) {
    let ansi = *ANSI.get().unwrap();
    if ansi {
        println!("\x1b[1;94mi) \x1b[0m{}", message);
    } else {
        println!("i) {}", message);
    }
}

pub fn _error(message: String) {
    let ansi = *ANSI.get().unwrap();
    if ansi {
        eprintln!("\x1b[1;91mX) \x1b[0m{}", message);
    } else {
        eprintln!("X) {}", message);
    }
}

pub fn _warn(message: String) {
    let ansi = *ANSI.get().unwrap();
    if ansi {
        eprintln!("\x1b[1;93m!) \x1b[0m{}", message);
    } else {
        eprintln!("!) {}", message);
    }
}

pub fn _question(message: String) -> String {
    let ansi = *ANSI.get().unwrap();
    if ansi {
        format!("\x1b[1;90m?) \x1b[0m{}", message)
    } else {
        format!("?) {}", message)
    }
}
