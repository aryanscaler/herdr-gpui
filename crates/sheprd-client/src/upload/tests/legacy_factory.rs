// Keep the existing protocol fixtures infallible; route tests exercise factory
// failures through the public API.
fn upload(
    paths: &[PathBuf],
    cancelled: &AtomicBool,
    progress: impl FnMut(u64, u64),
    mut command: impl FnMut(&str) -> Command,
) -> Result<Vec<String>> {
    super::upload(paths, cancelled, progress, |script| Ok(command(script)))
}

fn cleanup(owned: &[StagedUpload], command: &mut impl FnMut(&str) -> Command) -> Result<()> {
    super::cleanup(owned, &mut |script| Ok(command(script)))
}
