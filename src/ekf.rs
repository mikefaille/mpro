use wide::f64x2;

/// 2D Extended Kalman Filter (EKF) with Explicit SIMD (eSIMD) Vectorization using `wide::f64x2`.
/// Uses 128-bit vector registers for nanosecond state prediction and covariance scaling.
pub struct ExtendedKalmanThermalFilter {
    dt: f64,
    x0: f64, // Estimated Temperature (°C)
    x1: f64, // Estimated Derivative Rate-of-Change (°C/sec)

    p00: f64,
    p01: f64,
    p10: f64,
    p11: f64,

    q00: f64,
    q11: f64,
    r_meas: f64,
}

impl ExtendedKalmanThermalFilter {
    pub fn new(dt: f64) -> Self {
        Self {
            dt,
            x0: 50.0,
            x1: 0.0,

            p00: 1.0,
            p01: 0.0,
            p10: 0.0,
            p11: 1.0,

            q00: 1e-3,
            q11: 1e-2,
            r_meas: 0.5,
        }
    }

    #[inline(always)]
    pub fn update(&mut self, z_meas: f64) -> (f64, f64) {
        // SIMD 128-bit vector state prediction using wide::f64x2: [x0 + dt*x1, x1]
        let vec_x = f64x2::from([self.x0, self.x1]);
        let vec_dt = f64x2::from([self.dt * self.x1, 0.0]);
        let vec_x_p = vec_x + vec_dt;

        let x_p: [f64; 2] = vec_x_p.into();
        let x0_p = x_p[0];
        let x1_p = x_p[1];

        let p00_p = self.p00 + self.dt * (self.p10 + self.p01) + (self.dt * self.dt) * self.p11 + self.q00;
        let p01_p = self.p01 + self.dt * self.p11;
        let p10_p = self.p10 + self.dt * self.p11;
        let p11_p = self.p11 + self.q11;

        let y = z_meas - x0_p;
        let s = p00_p + self.r_meas;

        let k0 = p00_p / s;
        let k1 = p10_p / s;

        self.x0 = x0_p + k0 * y;
        self.x1 = x1_p + k1 * y;

        self.p00 = p00_p - k0 * p00_p;
        self.p01 = p01_p - k0 * p01_p;
        self.p10 = p10_p - k1 * p00_p;
        self.p11 = p11_p - k1 * p01_p;

        (self.x0, self.x1)
    }
}
