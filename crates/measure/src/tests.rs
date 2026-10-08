use super::*;
use pdfcraft_cos::{SaveOptions, write_full};
use std::sync::Arc;
fn fixture(content: &str, attrs: &str, extra: &[&str]) -> Document {
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612 792] >>".into(),
        format!("<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /XObject << /Nested 5 0 R >> >> {attrs} >>"),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
    ];
    objects.extend(extra.iter().map(|s| s.to_string()));
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = bytes.len();
    bytes.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for o in offsets {
        bytes.extend(format!("{o:010} 00000 n \n").as_bytes());
    }
    bytes.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).as_bytes());
    Document::open(Arc::new(bytes)).unwrap()
}
fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-8, "{a} != {b}");
}
fn new(kind: Kind, points: Vec<Point>) -> NewMeasurement {
    NewMeasurement {
        page: 0,
        kind,
        points,
        scale: Scale::new(2.0, "m", 3).unwrap(),
        style: Style { color: [0.0, 0.47, 0.84], ..Style::default() },
        label: "Room 1".into(),
        author: "A".into(),
    }
}
#[test]
fn calibrated_lengths_concave_areas_and_axis_scales() {
    let scale = Scale::calibrate([20.0, 30.0], [23.0, 34.0], 10.0, "m", 3).unwrap();
    let distance = reading(Kind::Distance, &[[20.0, 30.0], [23.0, 34.0]], &scale).unwrap();
    close(distance.value, 10.0);
    close(distance.delta_x, 6.0);
    close(distance.delta_y, 8.0);
    assert_eq!(distance.label, "10.000 m");
    let path = [[0.0, 0.0], [3.0, 0.0], [3.0, 4.0], [0.0, 0.0]];
    close(reading(Kind::Perimeter, &path, &scale).unwrap().value, 24.0);
    let concave = [[1e8, 1e8], [1e8 + 3.0, 1e8], [1e8 + 3.0, 1e8 + 1.0], [1e8 + 1.0, 1e8 + 1.0], [1e8 + 1.0, 1e8 + 3.0], [1e8, 1e8 + 3.0]];
    close(reading(Kind::Area, &concave, &scale).unwrap().value, 20.0);
    let reversed: Vec<_> = concave.into_iter().rev().collect();
    close(reading(Kind::Area, &reversed, &scale).unwrap().value, 20.0);
    let anisotropic = Scale { y: 3.0, ..Scale::new(2.0, "m", 2).unwrap() };
    close(reading(Kind::Distance, &[[0.0, 0.0], [3.0, 4.0]], &anisotropic).unwrap().value, 180f64.sqrt());
    close(reading(Kind::Area, &[[0.0, 0.0], [3.0, 0.0], [3.0, 4.0]], &anisotropic).unwrap().value, 36.0);
}
#[test]
fn viewport_precedence_first_point_user_unit_and_roundtrip() {
    let mut d = fixture("", "/UserUnit 2 /Rotate 90 /CropBox [10 20 210 320]", &[]);
    close(scale_at(&d, 0, [50.0, 50.0]).unwrap().x, 2.0 / 72.0);
    set_scale(&mut d, 0, [10.0, 20.0, 210.0, 320.0], "large", &Scale::new(1.0, "m", 2).unwrap()).unwrap();
    set_scale(&mut d, 0, [20.0, 30.0, 50.0, 60.0], "detail", &Scale::new(10.0, "m", 4).unwrap()).unwrap();
    close(scale_at(&d, 0, [25.0, 35.0]).unwrap().x, 10.0);
    close(scale_at(&d, 0, [100.0, 100.0]).unwrap().x, 1.0);
    // An annotation carries the chosen scale even when its second point leaves the viewport.
    let mut m = new(Kind::Distance, vec![[25.0, 35.0], [100.0, 35.0]]);
    m.scale = scale_at(&d, 0, m.points[0]).unwrap();
    add(&mut d, &m, &Meta { date: None, id: "measure".into() }).unwrap();
    let bytes = write_full(&d, &SaveOptions::default()).unwrap();
    let reopened = Document::open(Arc::new(bytes)).unwrap();
    let measurements = list(&reopened).unwrap();
    close(measurements[0].reading.value, 750.0);
    assert_eq!(measurements[0].id, "measure");
    assert_eq!(scale_at(&reopened, 0, [25.0, 35.0]).unwrap().precision, 4);
}
#[test]
fn standard_annotations_captions_restyle_move_and_unknown_keys() {
    let mut d = fixture("", "", &[]);
    let meta = Meta { date: None, id: String::new() };
    for m in [
        new(Kind::Distance, vec![[0.0, 0.0], [3.0, 4.0]]),
        new(Kind::Perimeter, vec![[20.0, 0.0], [23.0, 0.0], [23.0, 4.0]]),
        new(Kind::Area, vec![[40.0, 0.0], [43.0, 0.0], [43.0, 4.0]]),
    ] {
        add(&mut d, &m, &meta).unwrap();
    }
    assert_eq!(list(&d).unwrap().iter().map(|m| m.reading.value).collect::<Vec<_>>(), vec![10.0, 14.0, 24.0]);
    let (r, dict) = annot(&d, 0, 0).unwrap();
    assert_eq!(dict.name(b"IT"), Some(&b"LineDimension"[..]));
    d.update_dict(r, |d| d.set(b"VendorKey".to_vec(), Object::Int(42))).unwrap();
    pdfcraft_annot::set_style(&mut d, 0, 0, Some([1.0, 0.0, 0.0]), None, Some(2.0), &meta).unwrap();
    let (_, dict) = annot(&d, 0, 0).unwrap();
    assert_eq!(dict.int(b"VendorKey"), Some(42));
    let ap = d.resolve(dict.get(b"AP").unwrap());
    let normal = d.resolve(ap.as_dict().unwrap().get(b"N").unwrap());
    let Object::Stream(stream) = normal.as_ref() else { panic!("appearance missing") };
    let stream = String::from_utf8(stream.decoded().unwrap()).unwrap();
    assert!(stream.contains("(10.000 m) Tj"), "{stream}");
    pdfcraft_annot::move_annotation(&mut d, 0, 0, 50.0, 60.0, &meta).unwrap();
    assert_eq!(list(&d).unwrap()[0].points, vec![[50.0, 60.0], [53.0, 64.0]]);
    close(list(&d).unwrap()[0].reading.value, 10.0);
}
#[test]
fn rejects_invalid_geometry_and_unsupported_scales() {
    let mut d = fixture("", "", &[]);
    for points in [vec![[f64::NAN, 0.0], [2.0, 0.0]], vec![[0.0, 0.0]; 2], vec![[0.0, 0.0], [1e10, 0.0]]] {
        assert!(add(&mut d, &new(Kind::Distance, points), &Meta::default()).is_err());
    }
    assert!(add(&mut d, &new(Kind::Area, vec![[0.0, 0.0], [2.0, 2.0], [0.0, 2.0], [2.0, 0.0]]), &Meta::default()).is_err());
    assert!(Scale::new(f64::INFINITY, "m", 2).is_err());
    assert!(Scale::new(1.0, "m", 7).is_err());
    assert!(Scale::calibrate([0.0; 2], [0.0; 2], 1.0, "m", 2).is_err());
    let mut dict = Scale::default().dictionary().unwrap();
    dict.set(b"Subtype".to_vec(), Object::name("GEO"));
    assert!(Scale::read(&d, &Object::Dict(dict)).unwrap_err().to_string().contains("rectilinear"));
    assert!(set_scale(&mut d, 0, [0.0; 4], "bad", &Scale::default()).is_err());
}
#[test]
fn snap_paths_endpoints_midpoints_and_intersections() {
    let d = fixture("10 20 m 110 20 l S 60 0 m 60 80 l S 10 100 100 40 re S", "", &[]);
    let paths = snap::geometry(&d, 0).unwrap();
    assert_eq!(paths.segments.len(), 6);
    let opts = snap::SnapOptions::default();
    let hit = paths.snap([11.0, 20.0], 3.0, opts).unwrap().unwrap();
    assert_eq!(hit.kind, snap::SnapKind::Endpoint);
    assert_eq!(hit.point, [10.0, 20.0]);
    let hit = paths.snap([60.0, 41.0], 3.0, opts).unwrap().unwrap();
    assert_eq!(hit.kind, snap::SnapKind::Midpoint);
    assert_eq!(hit.point, [60.0, 40.0]);
    let hit = paths.snap([59.0, 21.0], 3.0, snap::SnapOptions { midpoints: false, ..opts }).unwrap().unwrap();
    assert_eq!(hit.kind, snap::SnapKind::Intersection);
    assert_eq!(hit.point, [60.0, 20.0]);
    let hit = paths.snap([32.0, 22.0], 3.0, opts).unwrap().unwrap();
    assert_eq!(hit.kind, snap::SnapKind::Path);
    assert_eq!(hit.point, [32.0, 20.0]);
    assert!(paths.snap([200.0, 200.0], 3.0, opts).unwrap().is_none());
    assert!(paths.snap([f64::NAN, 0.0], 3.0, opts).is_err());
}
#[test]
fn nested_forms_transforms_curves_and_cycles_are_bounded() {
    let body = "0 0 m 10 0 l S 0 0 m 0 10 10 10 10 0 c S /Nested Do";
    let form = format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 10 10] /Matrix [2 0 0 3 0 0] /Resources << /XObject << /Nested 5 0 R >> >> /Length {} >>\nstream\n{body}\nendstream",
        body.len()
    );
    let d = fixture("q 1 0 0 1 100 200 cm /Nested Do Q", "", &[&form]);
    let geometry = snap::geometry(&d, 0).unwrap();
    assert_eq!(geometry.segments[0], [[100.0, 200.0], [120.0, 200.0]]);
    assert!(geometry.endpoints.contains(&[120.0, 200.0]));
    assert!(geometry.midpoints.contains(&[110.0, 222.5]));
    assert!(geometry.segments.len() > 4 && geometry.segments.len() < 200);
    let d = fixture(&"0 0 m 1 1 l S ".repeat(20_001), "", &[]);
    assert!(snap::geometry(&d, 0).unwrap().truncated);
}
#[test]
fn measurement_csv_quotes_newlines_and_blocks_formula_cells() {
    let mut d = fixture("", "", &[]);
    let mut m = new(Kind::Distance, vec![[0.0, 0.0], [3.0, 4.0]]);
    m.label = "=SUM(1,2)\n\"room\"".into();
    m.author = "  @cmd".into();
    add(&mut d, &m, &Meta::default()).unwrap();
    let csv = csv(&list(&d).unwrap());
    assert!(csv.contains("\"'=SUM(1,2)\n\"\"room\"\"\""));
    assert!(csv.contains("\"'  @cmd\""));
    assert!(csv.starts_with("Page,Index,Type,Value"));
}

#[test]
fn coordinates_roundtrip_for_crop_rotation_and_user_unit() {
    for rotation in [0, 90, 180, 270] {
        let d = fixture("", &format!("/UserUnit 2 /Rotate {rotation} /CropBox [10 20 210 320]"), &[]);
        for view in [[0.0, 0.0], [37.125, 81.375], [123.0, 175.0]] {
            let user = view_to_user(&d, 0, view).unwrap();
            let actual = user_to_view(&d, 0, user).unwrap();
            close(actual[0], view[0]);
            close(actual[1], view[1]);
        }
        let a = view_to_user(&d, 0, [10.0, 20.0]).unwrap();
        let b = view_to_user(&d, 0, [70.0, 100.0]).unwrap();
        let scale = scale_at(&d, 0, a).unwrap();
        close(reading(Kind::Distance, &[a, b], &scale).unwrap().value, 100.0 / 72.0);
    }
}

#[test]
fn zero_decimal_scale_uses_standard_rounding_format() {
    let d = fixture("", "", &[]);
    let scale = Scale::new(1.0, "m", 0).unwrap();
    let dict = scale.dictionary().unwrap();
    let first = dict.get(b"D").unwrap().as_array().unwrap()[0].as_dict().unwrap();
    assert_eq!(first.name(b"F"), Some(&b"R"[..]));
    assert_eq!(Scale::read(&d, &Object::Dict(dict)).unwrap(), scale);
}

#[test]
fn imported_measurement_appearance_is_not_silently_replaced() {
    let mut d = fixture("", "", &[]);
    add(&mut d, &new(Kind::Distance, vec![[10.0, 20.0], [40.0, 60.0]]), &Meta::default()).unwrap();
    let (r, dict) = annot(&d, 0, 0).unwrap();
    let original = dict.get(b"AP").unwrap().clone();
    d.update_dict(r, |d| {
        d.remove(b"PCMeasureValue");
    })
    .unwrap();
    assert!(pdfcraft_annot::set_appearance(&mut d, r).is_err());
    assert_eq!(annot(&d, 0, 0).unwrap().1.get(b"AP"), Some(&original));
    close(list(&d).unwrap()[0].reading.value, 100.0);
}
