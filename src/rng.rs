/// Small xorshift64* generator; no need for a crate.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed ^ 0x9E37_79B9_7F4A_7C15 | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in [0, 1).
    pub fn f(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f()
    }

    pub fn below(&mut self, n: u32) -> u32 {
        (self.next_u64() % n as u64) as u32
    }

    /// Integer in [lo, hi] returned as f32, for shader params that must be whole.
    pub fn int(&mut self, lo: i32, hi: i32) -> f32 {
        (lo + self.below((hi - lo + 1) as u32) as i32) as f32
    }

    pub fn chance(&mut self, p: f32) -> bool {
        self.f() < p
    }

    pub fn sign(&mut self) -> f32 {
        if self.chance(0.5) {
            1.0
        } else {
            -1.0
        }
    }
}
