//! Tracing of the main phases of a layout, on standard error.
//!
//! Nothing is printed unless `NLTT_TRACE` is set to something other than `0`, `no`, `off`
//! or `false`. The variable is read once per process and cached, so the instrumentation
//! costs one load per phase — never one per node, and nothing at all inside the contour
//! walk, which is where the time goes.
//!
//! ```text
//! $ NLTT_TRACE=1 cargo run --example trace
//! [nltt] layout       root=#1 arena=4 vertically=true centeredxy=false origin=(0, 0) hooks=none
//! [nltt]   setup      875.000ns  nodes=4 depth=3
//! [nltt]   first        2.500µs
//! [nltt]   second     875.000ns  minbreadth=0
//! [nltt]   third       83.000ns  already at the origin
//! [nltt]   total        4.333µs
//! [nltt] layout_flat  root=#1 arena=4 vertically=true centeredxy=false origin=(0, 0) kernels=scalar
//! [nltt]   build        4.917µs  nodes=4 depth=3
//! [nltt]   setup        1.416µs
//! [nltt]   first        2.833µs
//! [nltt]   second       2.792µs  minbreadth=0
//! [nltt]   third       42.000ns  already at the origin
//! [nltt]   write      833.000ns  4 nodes
//! [nltt]   total       12.833µs
//! ```
//!
//! The phase names are the ones the sources use: `setup`, `first`, `second` and `third`
//! are the walks of [`crate::layout_with`] — the sweeps, in [`crate::flat`] — while
//! `build` and `write` are the mirror being filled and read back. `total` is the sum of
//! the phases and not the wall clock, which for a small tree is dominated by the trace
//! writing these very lines.
//!
//! [`crate::flat::Engine::profile`] reports the same phases as a value rather than as a
//! line, for a caller that wants to tabulate them.

use std::fmt;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// Whether `NLTT_TRACE` asks for the phase logs.
///
/// The environment is read once, on the first call, and cached from then on.
pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();

    *ON.get_or_init(|| match std::env::var("NLTT_TRACE") {
        Ok(v) => asked_for(&v),
        Err(_) => false,
    })
}

/// Whether a value of `NLTT_TRACE` turns the logs on.
fn asked_for(v: &str) -> bool {
    !matches!(v.trim(), "" | "0" | "no" | "off" | "false")
}

/// A stopwatch, when tracing is on; `None` otherwise, which is what keeps the
/// `phase!` arguments from being evaluated at all.
pub(crate) fn start() -> Option<Instant> {
    enabled().then(Instant::now)
}

/// One unindented line, for the header of a layout.
pub(crate) fn header(args: fmt::Arguments) {
    eprintln!("[nltt] {args}");
}

/// One indented line for a phase that took `d`.
pub(crate) fn line(name: &str, d: Duration, args: fmt::Arguments) {
    eprintln!("{}", phase_line(name, d, args));
}

/// The text of a phase line; [`line`] is this, on standard error.
fn phase_line(name: &str, d: Duration, args: fmt::Arguments) -> String {
    let detail = format!("{args}");
    let sep = if detail.is_empty() { "" } else { "  " };

    format!("[nltt]   {name:<7} {d:>12.3?}{sep}{detail}")
}

/// A header line, when tracing is on.
///
/// The arguments are only evaluated then, so anything the trace has to walk the tree for
/// costs nothing when it is off.
macro_rules! trace {
    ($($arg:tt)*) => {
        if $crate::trace::enabled() {
            $crate::trace::header(format_args!($($arg)*));
        }
    };
}

/// Closes the phase started by [`start`], printing how long it took and
/// evaluating to that duration, so that a caller can sum the phases into a total.
///
/// `phase!(t, "first")` prints the name and the duration alone; any further arguments are
/// a `format!` for the detail that follows them. With tracing off the phase is
/// [`Duration::ZERO`] and nothing else happens.
macro_rules! phase {
    ($t:expr, $name:expr $(,)?) => {
        phase!($t, $name, "")
    };
    ($t:expr, $name:expr, $($arg:tt)*) => {
        match $t {
            Some(__t) => {
                let __d = __t.elapsed();
                $crate::trace::line($name, __d, format_args!($($arg)*));
                __d
            }
            None => std::time::Duration::ZERO,
        }
    };
}

/// The closing line of a traced layout: what the phases add up to.
///
/// The wall clock would include the trace's own writes to standard error, which for a
/// small tree are longer than the layout; summing the phases leaves them out.
macro_rules! total {
    ($on:expr, $sum:expr) => {
        if $on {
            $crate::trace::line("total", $sum, format_args!(""));
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_turns_the_logs_on() {
        for v in ["1", "yes", "true", "on", "anything"] {
            assert!(asked_for(v), "`{v}` asks for the logs");
        }

        for v in ["", "  ", "0", "no", "off", "false", " 0 "] {
            assert!(!asked_for(v), "`{v}` does not ask for the logs");
        }
    }

    #[test]
    fn a_phase_line_is_padded_and_its_detail_optional() {
        let d = Duration::from_micros(1234);

        assert_eq!(
            phase_line("first", d, format_args!("")),
            "[nltt]   first        1.234ms"
        );
        assert_eq!(
            phase_line("second", d, format_args!("minbreadth={}", -15.0)),
            "[nltt]   second       1.234ms  minbreadth=-15"
        );
    }
}
