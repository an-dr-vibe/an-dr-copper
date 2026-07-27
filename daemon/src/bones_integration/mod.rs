//! External Bones modules and composition helpers owned by Copper.

mod capability;
mod control;
mod driver;
mod jobs;
mod platform_jobs;
mod protocol;

pub use capability::{
    AuthorizedCapabilityJob, CopperCapabilityHandle, CopperCapabilityModule,
    COPPER_CAPABILITY_ENDPOINT,
};
pub use control::{
    CopperControlHandle, CopperControlModule, CopperLifecycleState, COPPER_CONTROL_ENDPOINT,
};
pub use driver::{
    execute_component_action, BonesActionDispatch, BonesDaemonDriver, BonesRuntimeStatus,
    COPPER_ACTION_SENDER,
};
pub use protocol::{Capability, CopperEnvelope, COPPER_BUS_PROTOCOL_V1};
