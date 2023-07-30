use intertrait::CastFrom;
use intertrait::cast::CastRef;
use strum::{EnumIter, IntoEnumIterator};

pub trait Capability : CastFrom {
    fn get_capabilities(&self) -> Vec<CapabilityId> {
        let mut capabilities = Vec::<CapabilityId>::new();
        for capability in CapabilityId::iter() {
            let has_capability = match capability {

            };

            if has_capability {
                capabilities.push(capability);
            }
        }

        capabilities
    }
}

#[derive(Debug, EnumIter, Clone)]
pub enum CapabilityId {
    
}

// Any capability APIs will go here