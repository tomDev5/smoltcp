// #[cfg(feature = "std")]
// mod dispatch_table;
// #[cfg(feature = "std")]
// pub(crate) use self::dispatch_table::DispatchTable;
// #[cfg(not(feature = "std"))]
mod dispatch_table_nostd;
// #[cfg(not(feature = "std"))]
pub(crate) use self::dispatch_table_nostd::{DispatchTable, Error};
