//! Safe boundary for the host's Windows volume-label FFI.
/// The host supplies labels using the frozen GetVolumeInformationW behavior.
pub trait VolumeLabelProvider: Send + Sync {
    fn volume_label(&self, drive: &str)
        -> Result<String, Box<dyn std::error::Error + Send + Sync>>;
}
impl<F> VolumeLabelProvider for F
where
    F: Send + Sync + Fn(&str) -> Result<String, Box<dyn std::error::Error + Send + Sync>>,
{
    fn volume_label(
        &self,
        drive: &str,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        self(drive)
    }
}
