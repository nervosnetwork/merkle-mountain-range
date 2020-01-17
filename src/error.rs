pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Error {
    GetRootOnEmpty,
    InconsistentStore,
    StoreError(crate::string::String),
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        use Error::*;
        match self {
            GetRootOnEmpty => write!(f, "Get root on an empty MMR")?,
            InconsistentStore => write!(f, "Inconsistent store")?,
            StoreError(msg) => write!(f, "Store error {}", msg)?,
        }
        Ok(())
    }
}

cfg_if::cfg_if! {
    if #[cfg(feature = "std")] {
        impl ::std::error::Error for Error {}
    }
}
