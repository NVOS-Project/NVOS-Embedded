use tonic::Status;
use crate::device::DeviceError;

pub fn map_device_error(err: DeviceError) -> Status {
    match err {
        DeviceError::NotFound(_) => Status::not_found(err.to_string()),
        DeviceError::MissingController(_) => Status::unavailable(err.to_string()),
        DeviceError::DuplicateController => Status::already_exists(err.to_string()),
        DeviceError::DuplicateDevice(_) => Status::already_exists(err.to_string()),
        DeviceError::HardwareError(_) => Status::internal(err.to_string()),
        DeviceError::InvalidOperation(_) => Status::failed_precondition(err.to_string()),
        DeviceError::InvalidConfig(_) => Status::invalid_argument(err.to_string()),
        DeviceError::NotSupported => Status::unimplemented(err.to_string()),
        DeviceError::Internal => Status::internal(err.to_string()),
        DeviceError::Other(_) => Status::unknown(err.to_string()),
    }
}