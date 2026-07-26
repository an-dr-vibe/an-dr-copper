//! External Bones modules and composition helpers owned by Copper.

mod control;
mod driver;

pub use control::{CopperControlHandle, CopperControlModule, COPPER_CONTROL_ENDPOINT};
pub use driver::{BonesDaemonDriver, BonesRuntimeStatus};
