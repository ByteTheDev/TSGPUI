// Rendrix patch: libc no longer defines ENOATTR on Linux. Linux reports a missing attribute as
// ENODATA, so use that value there. Other platforms keep libc's ENOATTR.
#[cfg(any(target_os = "linux", target_os = "android"))]
const ENOATTR_VALUE: ::libc::c_int = ::libc::ENODATA;
#[cfg(not(any(target_os = "linux", target_os = "android")))]
const ENOATTR_VALUE: ::libc::c_int = ::libc::ENOATTR;

macro_rules! platforms {
    ($($($platform:expr);* => $module:ident),*) => {
        $(
            #[cfg(any($(target_os = $platform),*))]
            #[cfg_attr(not(any($(target_os = $platform),*)), allow(dead_code))]
            mod $module;

            #[cfg(any($(target_os = $platform),*))]
            pub use self::$module::*;

            #[cfg(any($(target_os = $platform),*))]
            pub const ENOATTR: ::libc::c_int = ENOATTR_VALUE;
        )*

        #[cfg(all(feature = "unsupported", not(any($($(target_os = $platform),*),*))))]
        #[cfg_attr(any($($(target_os = $platform),*),*), allow(dead_code))]
        mod unsupported;

        #[cfg(all(feature = "unsupported", not(any($($(target_os = $platform),*),*))))]
        pub use self::unsupported::*;
        #[cfg(all(feature = "unsupported", not(any($($(target_os = $platform),*),*))))]
        pub const ENOATTR: ::libc::c_int = 0;


        /// A constant indicating whether or not the target platform is supported.
        ///
        /// To make programmer's lives easier, this library builds on all platforms.
        /// However, all function calls on unsupported platforms will return
        /// `io::Error`s.
        ///
        /// Note: If you would like compilation to simply fail on unsupported platforms,
        /// turn of the `unsupported` feature.
        pub const SUPPORTED_PLATFORM: bool = cfg!(any($($(target_os = $platform),*),*));
    }
}

platforms! {
    "android"; "linux"; "macos" => linux_macos,
    "freebsd"; "netbsd" => bsd
}
