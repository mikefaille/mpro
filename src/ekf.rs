use wide::f64x2;

/// 2D Extended Kalman Filter (EKF) with Full eSIMD Vectorization.
/// Uses 128-bit vector registers (wide::f64x2) for state vector prediction and covariance updates.
pub struct ExtendedKalmanThermalFilter {
    dt: f64,
    x0: f64, // Estimated Temperature (°C)
    x1: f64, // Estimated Derivative Rate-of-Change (°C/sec)

    p_row0: f64x2, // [p00, p01]
    p_row1: f64x2, // [p10, p11]

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

            p_row0: f64x2::from([1.0, 0.0]),
            p_row1: f64x2::from([0.0, 1.0]),

            q00: 1e-3,
            q11: 1e-2,
            r_meas: 0.5,
        }
    }

    #[inline(always)]
    pub fn update(&mut self, z_meas: f64) -> (f64, f64) {
        // 1. eSIMD State Prediction: [x0 + dt*x1, x1]
        let vec_x = f64x2::from([self.x0, self.x1]);
        let vec_dt = f64x2::from([self.dt * self.x1, 0.0]);
        let vec_x_p = vec_x + vec_dt;

        let x_p: [f64; 2] = vec_x_p.into();
        let x0_p = x_p[0];

        // Extract covariance terms
        let p: [f64; 2] = self.p_row0.into();
        let p00 = p[0];
        let p01 = p[1];

        let p1: [f64; 2] = self.p_row1.into();
        let p10 = p1[0];
        let p11 = p1[1];

        // 2. Fused Vector Covariance Prediction
        let dt_vec = f64x2::splat(self.dt);
        let p1_vec = f64x2::from([p10, p11]);

        let p0_pred = f64x2::from([p00, p01]) + dt_vec * p1_vec + f64x2::from([self.q00, 0.0]);
        let p1_pred = p1_vec + f64x2::from([0.0, self.q11]);

        let p0_arr: [f64; 2] = p0_pred.into();
        let p00_p = p0_arr[0];

        // 3. Measurement Update & Kalman Gain
        let y = z_meas - x0_p;
        let s = p00_p + self.r_meas;
        let inv_s = 1.0 / s;

        let k0 = p00_p * inv_s;
        let k1 = p10 * inv_s;

        // 4. eSIMD State Correction
        let k_vec = f64x2::from([k0, k1]);
        let y_vec = f64x2::splat(y);
        let vec_x_corrected = vec_x_p + k_vec * y_vec;

        let x_corr: [f64; 2] = vec_x_corrected.into();
        self.x0 = x_corr[0];
        self.x1 = x_corr[1];

        // Update Covariance Vector Lanes
        self.p_row0 = p0_pred * (f64x2::splat(1.0) - k_vec);
        self.p_row1 = p1_pred * (f64x2::splat(1.0) - k_vec);

        (self.x0, self.x1)
    }
}
