//! Preserve the Linux model-only crossing while macOS/Windows use native providers.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub use crate::platform::accessibility::NativeBridge;

// There is no Linux native provider in the committed provider API. The Host sets
// native=false on that target; this fail-closed seam preserves compilation without
// pretending to expose an OS accessibility endpoint.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod unsupported {
    use crate::platform::accessibility::{
        ActionSink, PublishedOutcome, Rejection, RetiredPublication,
        model::{FrameKey, MaterializedNodes, PreparedFrame},
    };
    use std::sync::Arc;
    pub struct NativeBridge;
    impl NativeBridge {
        pub fn attach_unpublished(
            _: &impl raw_window_handle::HasWindowHandle,
            _: ActionSink,
        ) -> Result<Self, Rejection> {
            Err(Rejection::Unsupported)
        }
        pub fn materialized_nodes(&self) -> Arc<MaterializedNodes> {
            Arc::new(MaterializedNodes::default())
        }
        pub fn invalidate_geometry(&mut self) -> Result<(), Rejection> {
            Err(Rejection::Unsupported)
        }
        pub fn publish_prepared(
            &mut self,
            _: Arc<PreparedFrame>,
            _: FrameKey,
        ) -> Result<PublishedOutcome<Rejection>, Rejection> {
            Err(Rejection::Unsupported)
        }
        pub fn detach_prepared(&mut self) -> Result<RetiredPublication, Rejection> {
            Err(Rejection::Unsupported)
        }
        pub fn drain_native_retirement(&mut self, _: usize) -> Result<usize, Rejection> {
            Err(Rejection::Unsupported)
        }
        pub fn native_retirement_complete(&self) -> bool {
            false
        }
    }
}
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub use unsupported::NativeBridge;
