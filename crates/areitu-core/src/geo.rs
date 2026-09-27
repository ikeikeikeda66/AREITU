const EARTH_RADIUS_M: f64 = 6_371_000.0;

pub fn haversine_m(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (lat1, lon1) = (a.0.to_radians(), a.1.to_radians());
    let (lat2, lon2) = (b.0.to_radians(), b.1.to_radians());
    let h = ((lat2 - lat1) / 2.0).sin().powi(2)
        + lat1.cos() * lat2.cos() * ((lon2 - lon1) / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_M * h.sqrt().asin()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokyo_to_shinjuku_is_about_6km() {
        let d = haversine_m((35.681236, 139.767125), (35.690921, 139.700258));
        assert!((d - 6_140.0).abs() < 100.0, "{d}");
    }

    #[test]
    fn same_point_is_zero() {
        assert_eq!(haversine_m((35.0, 139.0), (35.0, 139.0)), 0.0);
    }
}
