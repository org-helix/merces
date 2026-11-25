//! Keplerian orbits: slightly elliptical, slightly inclined, all prograde.
//!
//! Positions are a pure function of the simulation time, so the motion is the
//! same at 30, 60 or 144 FPS. Periods follow Kepler's third law
//! (`T² ∝ a³`): inner agents go round faster, and every agent speeds up near
//! the star (periapsis).

use std::f32::consts::TAU;

use bevy::math::{Quat, Vec3};

/// Period of an orbit with semi-major axis [`A_REF`], in simulation seconds.
const T_REF: f32 = 48.0;
const A_REF: f32 = 5.4;

/// The star's radius (the sun mesh), used to keep bodies apart from it.
pub const SUN_RADIUS: f32 = 1.7;

/// Lowest and highest camera pitch (radians above the orbital plane). Looking
/// down at least this much keeps the bodies of adjacent orbits apart on screen.
pub const PITCH_MIN: f32 = 0.85;
pub const PITCH_MAX: f32 = 1.4;

#[derive(Debug, Clone, Copy)]
pub struct Orbit {
    /// Semi-major axis.
    pub a: f32,
    /// Eccentricity.
    pub e: f32,
    /// Mean anomaly at t = 0.
    pub mean0: f32,
    /// Orbital period (simulation seconds).
    pub period: f32,
    /// Orbital plane → world: longitude of the ascending node, inclination,
    /// argument of periapsis.
    pub frame: Quat,
    /// Bounding radius of the body travelling on it.
    pub body_radius: f32,
    /// Spin period of the body around its own axis (simulation seconds).
    pub spin_period: f32,
}

impl Orbit {
    pub fn new(
        a: f32,
        e: f32,
        inclination_deg: f32,
        node_deg: f32,
        periapsis_deg: f32,
        mean0: f32,
        body_radius: f32,
    ) -> Self {
        let frame = Quat::from_rotation_y(node_deg.to_radians())
            * Quat::from_rotation_x(inclination_deg.to_radians())
            * Quat::from_rotation_y(periapsis_deg.to_radians());
        Self {
            a,
            e,
            mean0,
            period: T_REF * (a / A_REF).powf(1.5),
            frame,
            body_radius,
            spin_period: 14.0 + a,
        }
    }

    /// Closest and farthest distance to the star.
    #[cfg(test)]
    pub fn periapsis(&self) -> f32 {
        self.a * (1.0 - self.e)
    }
    #[cfg(test)]
    pub fn apoapsis(&self) -> f32 {
        self.a * (1.0 + self.e)
    }

    /// Point of the ellipse at eccentric anomaly `ecc` (prograde: counter-clockwise seen from +Y).
    pub fn at_eccentric_anomaly(&self, ecc: f32) -> Vec3 {
        let b = self.a * (1.0 - self.e * self.e).sqrt();
        let local = Vec3::new(self.a * (ecc.cos() - self.e), 0.0, -b * ecc.sin());
        self.frame * local
    }

    /// Mean anomaly at simulation time `t`.
    pub fn mean_anomaly(&self, t: f64) -> f32 {
        let turns = (t / f64::from(self.period)).fract() as f32;
        (self.mean0 + turns * TAU).rem_euclid(TAU)
    }

    pub fn position(&self, t: f64) -> Vec3 {
        self.at_eccentric_anomaly(solve_kepler(self.mean_anomaly(t), self.e))
    }

    /// Rotation of the body around its own axis at time `t` (radians).
    pub fn spin(&self, t: f64) -> f32 {
        ((t / f64::from(self.spin_period)).fract() as f32) * TAU
    }

    /// Normal of the orbital plane.
    #[cfg(test)]
    pub fn normal(&self) -> Vec3 {
        self.frame * Vec3::Y
    }
}

/// Solve Kepler's equation `M = E − e·sin E` for the eccentric anomaly `E`.
pub fn solve_kepler(mean: f32, e: f32) -> f32 {
    let mut ecc = if e < 0.8 { mean } else { std::f32::consts::PI };
    for _ in 0..8 {
        let f = ecc - e * ecc.sin() - mean;
        let step = f / (1.0 - e * ecc.cos());
        ecc -= step;
        if step.abs() < 1e-6 {
            break;
        }
    }
    ecc
}

/// The five orbits, indexed like `AgentId`: the four pipeline agents from the
/// inside out, then the assistant, farther and more inclined.
pub fn agent_orbits() -> [Orbit; 5] {
    [
        Orbit::new(4.8, 0.050, 1.2, 10.0, 30.0, 0.4, 0.8),
        Orbit::new(7.8, 0.035, 2.0, 70.0, 140.0, 2.3, 0.8),
        Orbit::new(10.7, 0.045, 1.5, 200.0, 250.0, 4.1, 0.8),
        Orbit::new(13.8, 0.040, 2.4, 310.0, 330.0, 5.6, 0.8),
        Orbit::new(17.8, 0.070, 5.0, 120.0, 60.0, 1.2, 0.65),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kepler_equation_is_solved() {
        for e in [0.0, 0.05, 0.3, 0.7] {
            for i in 0..64 {
                let m = i as f32 / 64.0 * TAU;
                let ecc = solve_kepler(m, e);
                assert!((ecc - e * ecc.sin() - m).abs() < 1e-4, "e={e} m={m}");
            }
        }
    }

    #[test]
    fn periods_follow_keplers_third_law() {
        let o = agent_orbits();
        for pair in o.windows(2) {
            let ratio = (pair[1].period / pair[0].period).powi(2);
            let expected = (pair[1].a / pair[0].a).powi(3);
            assert!((ratio - expected).abs() / expected < 1e-3);
            assert!(pair[1].period > pair[0].period, "outer orbits are slower");
        }
    }

    #[test]
    fn faster_near_the_star() {
        let o = agent_orbits()[0];
        let speed_at = |t: f64| (o.position(t + 0.01) - o.position(t)).length() / 0.01;
        // Periapsis: mean anomaly 0. Apoapsis: π.
        let t_peri = f64::from((TAU - o.mean0) / TAU * o.period);
        let t_apo = t_peri + f64::from(o.period) / 2.0;
        assert!(speed_at(t_peri) > speed_at(t_apo) * 1.05);
        assert!((o.position(t_peri).length() - o.periapsis()).abs() < 1e-3);
    }

    #[test]
    fn same_direction_and_slightly_inclined() {
        for o in agent_orbits() {
            assert!(o.normal().y > 0.97, "prograde, low inclination");
            assert!(o.e > 0.0 && o.e < 0.1, "slightly elliptical");
            // Angular momentum (r × v) points up for every orbit.
            let (r, v) = (o.position(1.0), o.position(1.01) - o.position(1.0));
            assert!(r.cross(v).y > 0.0);
        }
    }

    #[test]
    fn orbits_never_cross() {
        let o = agent_orbits();
        assert!(o[0].periapsis() - SUN_RADIUS > o[0].body_radius * 2.0);
        for pair in o.windows(2) {
            let gap = pair[1].periapsis() - pair[0].apoapsis();
            assert!(gap > pair[0].body_radius + pair[1].body_radius);
        }
    }

    /// Seen from any allowed camera direction, at any time, no two bodies (nor
    /// a body and the star) overlap on screen (orthographic approximation,
    /// with a 5 % margin for perspective).
    #[test]
    fn bodies_never_overlap_on_screen() {
        let o = agent_orbits();
        let longest = o.iter().map(|x| x.period).fold(0.0, f32::max);
        for yaw_i in 0..24 {
            for pitch_i in 0..=8 {
                let yaw = yaw_i as f32 / 24.0 * TAU;
                let pitch = PITCH_MIN + (PITCH_MAX - PITCH_MIN) * pitch_i as f32 / 8.0;
                let rot = Quat::from_rotation_y(yaw) * Quat::from_rotation_x(-pitch);
                let (right, up) = (rot * Vec3::X, rot * Vec3::Y);
                let screen = |p: Vec3| bevy::math::Vec2::new(p.dot(right), p.dot(up));
                for step in 0..1500 {
                    let t = f64::from(longest) * 2.0 * f64::from(step) / 1500.0;
                    let pos: Vec<_> = o.iter().map(|x| screen(x.position(t))).collect();
                    for i in 0..o.len() {
                        let to_sun = pos[i].length();
                        assert!(
                            to_sun > (SUN_RADIUS + o[i].body_radius) * 1.05,
                            "body {i} over the star (yaw {yaw:.2}, pitch {pitch:.2}, t {t:.1})"
                        );
                        for j in i + 1..o.len() {
                            let d = pos[i].distance(pos[j]);
                            assert!(
                                d > (o[i].body_radius + o[j].body_radius) * 1.05,
                                "bodies {i} and {j} overlap: {d:.2} (yaw {yaw:.2}, pitch {pitch:.2}, t {t:.1})"
                            );
                        }
                    }
                }
            }
        }
    }
}
