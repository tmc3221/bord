use super::effect::Effect;

/// Linear gain with dB control.
pub struct Gain {
    pub db: f32,
    lin: f32,
}

impl Gain {
    pub fn new(db: f32) -> Self {
        let mut g = Self { db, lin: 1.0 };
        g.recompute();
        g
    }
    fn recompute(&mut self) {
        self.lin = 10f32.powf(self.db / 20.0);
    }
}

impl Effect for Gain {
    fn set_param_db(&mut self, key: &str, db: f32) {
        if key == "db" {
            self.db = db;
            self.recompute();
        }
    }
    fn process(&mut self, block: &mut [f32]) {
        let g = self.lin;
        for s in block.iter_mut() {
            let x = *s * g;
            *s = if x > 1.0 { 1.0 } else if x < -1.0 { -1.0 } else { x };
        }
    }
}

