//! Macro-hidden `mod` recovery specimen.
//!
//! Multi-branch `cfg_if!` expands to the **union** of branches (matching
//! Horizon's recovery). `cfg_fs!` pastes items. `cfg_missing!` is a no-op so
//! the absent-file branch stays a Horizon-only recovery case. `stringify!`
//! must stay closed.

macro_rules! cfg_if {
    (
        if #[cfg($($cfg_a:tt)*)] { $($a:tt)* }
        else if #[cfg($($cfg_b:tt)*)] { $($b:tt)* }
        else { $($c:tt)* }
    ) => {
        $($a)*
        $($b)*
        $($c)*
    };
}

macro_rules! cfg_fs {
    ($($item:item)*) => {
        $($item)*
    };
}

macro_rules! cfg_missing {
    ($($tt:tt)*) => {};
}

cfg_if! {
    if #[cfg(feature = "net")] {
        mod net;
        pub use net::*;
    } else if #[cfg(feature = "alt")] {
        mod alt;
    } else {
        mod fallback;
    }
}

cfg_fs! {
    pub mod gated;
}

cfg_missing! {
    if #[cfg(feature = "missing_branch")] {
        mod absent_file;
    }
}

stringify!(mod must_not_appear;);

mod literal;

pub fn entry() -> u32 {
    net::net_fn()
        + alt::alt_fn()
        + fallback::fallback_fn()
        + gated::gated_fn()
        + literal::literal_fn()
}
