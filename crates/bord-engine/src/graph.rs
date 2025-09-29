use crate::dsp::effect::Effect;

/// A serial chain of effects. Owns the effects.
pub struct Chain {
    effects: Vec<Box<dyn Effect>>,
    channels: u16,
    sample_rate: u32,
}

impl Chain {
    pub fn new(sample_rate: u32, channels: u16) -> Self {
        Self { effects: Vec::new(), channels, sample_rate }
    }
    pub fn push(&mut self, mut fx: Box<dyn Effect>) {
        fx.prepare(self.sample_rate, self.channels);
        self.effects.push(fx);
    }
    /// Process one interleaved block in-place.
    pub fn process(&mut self, block: &mut [f32]) {
        for fx in self.effects.iter_mut() {
            fx.process(block);
        }
    }
}

