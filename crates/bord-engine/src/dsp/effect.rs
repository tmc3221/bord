/// Real-time safe effect interface.
/// - process() must not allocate or lock on the hot path.
/// - `block` is interleaved f32 samples in [-1, 1].
pub trait Effect: Send {
    fn prepare(&mut self, _sr: u32, _channels: u16) {}
    fn set_param_db(&mut self, _key: &str, _db: f32) {}
    fn process(&mut self, block: &mut [f32]);
}

