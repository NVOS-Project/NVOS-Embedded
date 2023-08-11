use tonic::{Response, Request, Status};

use self::heartbeat_server::Heartbeat;

use super::void::Void;

tonic::include_proto!("heartbeat");

pub struct HeartbeatService;

impl HeartbeatService {
    pub fn new() -> Self {
        Self {}
    }
}

#[tonic::async_trait]
impl Heartbeat for HeartbeatService {
    async fn ping(&self, _req: Request<Void>) -> Result<Response<Void>, Status> {
        Ok(Response::new(Void::default()))
    }
}