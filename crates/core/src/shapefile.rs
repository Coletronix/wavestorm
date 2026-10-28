//! A one-polygon ESRI shapefile (.shp, .shx, .dbf, .prj in WGS 84) packed in a zip archive:
//! the area of interest in the form Web Soil Survey's "Import AOI" accepts. Written from the
//! published ESRI Shapefile Technical Description (1998), dBASE III and PKWARE APPNOTE layouts.

use crate::geo::LonLat;

const PRJ_WGS84: &str = "GEOGCS[\"GCS_WGS_1984\",DATUM[\"D_WGS_1984\",SPHEROID[\"WGS_1984\",6378137.0,298.257223563]],PRIMEM[\"Greenwich\",0.0],UNIT[\"Degree\",0.0174532925199433]]";

fn be32(out: &mut Vec<u8>, v: i32) {
    out.extend_from_slice(&v.to_be_bytes());
}
fn le32(out: &mut Vec<u8>, v: i32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn le16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn f64le(out: &mut Vec<u8>, v: f64) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// The 100-byte main file header (lengths in 16-bit words).
fn header(out: &mut Vec<u8>, words: i32, bbox: [f64; 4]) {
    be32(out, 9994);
    for _ in 0..5 {
        be32(out, 0);
    }
    be32(out, words);
    le32(out, 1000);
    le32(out, 5); // polygon
    for v in bbox {
        f64le(out, v);
    }
    for _ in 0..4 {
        f64le(out, 0.0);
    }
}

/// The .shp and .shx files of one polygon ring (closed, clockwise as the format requires for
/// an outer ring).
fn shp_shx(ring: &[LonLat]) -> (Vec<u8>, Vec<u8>) {
    let mut pts = ring.to_vec();
    // Shoelace sum > 0 means counter-clockwise (x east, y north).
    let s: f64 = (0..pts.len())
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum();
    if s > 0.0 {
        pts.reverse();
    }
    if let Some(first) = pts.first().copied() {
        pts.push(first);
    }
    let bbox = pts.iter().fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |b, p| {
        [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])]
    });
    let mut content = Vec::new();
    le32(&mut content, 5);
    for v in bbox {
        f64le(&mut content, v);
    }
    le32(&mut content, 1);
    le32(&mut content, pts.len() as i32);
    le32(&mut content, 0);
    for p in &pts {
        f64le(&mut content, p[0]);
        f64le(&mut content, p[1]);
    }
    let content_words = (content.len() / 2) as i32;
    let mut shp = Vec::new();
    header(&mut shp, 50 + 4 + content_words, bbox);
    be32(&mut shp, 1);
    be32(&mut shp, content_words);
    shp.extend(content);
    let mut shx = Vec::new();
    header(&mut shx, 50 + 4, bbox);
    be32(&mut shx, 50);
    be32(&mut shx, content_words);
    (shp, shx)
}

/// A dBASE III table with one record and one numeric field ("ID" = 1).
fn dbf() -> Vec<u8> {
    let mut out = vec![0x03, 126, 10, 27];
    le32(&mut out, 1);
    le16(&mut out, 32 + 32 + 1);
    le16(&mut out, 1 + 10);
    out.extend([0u8; 20]);
    let mut name = [0u8; 11];
    name[..2].copy_from_slice(b"ID");
    out.extend(name);
    out.push(b'N');
    out.extend([0u8; 4]);
    out.push(10);
    out.push(0);
    out.extend([0u8; 14]);
    out.push(0x0D);
    out.push(b' ');
    out.extend(format!("{:>10}", 1).bytes());
    out.push(0x1A);
    out
}

/// CRC-32 (IEEE 802.3), as zip uses.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// A zip archive of uncompressed (stored) files.
pub fn zip_store(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in files {
        let offset = out.len() as u32;
        let crc = crc32(data);
        let entry = |out: &mut Vec<u8>, central_dir: bool| {
            out.extend_from_slice(&if central_dir { 0x0201_4b50u32 } else { 0x0403_4b50u32 }.to_le_bytes());
            if central_dir {
                le16(out, 20);
            }
            le16(out, 20); // version needed
            le16(out, 0); // flags
            le16(out, 0); // stored
            le16(out, 0); // time
            le16(out, 0x21); // date: 1980-01-01
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            le16(out, name.len() as u16);
            le16(out, 0); // extra
            if central_dir {
                le16(out, 0); // comment
                le16(out, 0); // disk
                le16(out, 0); // internal attributes
                le32(out, 0); // external attributes
                out.extend_from_slice(&offset.to_le_bytes());
            }
            out.extend_from_slice(name.as_bytes());
        };
        entry(&mut out, false);
        out.extend_from_slice(data);
        entry(&mut central, true);
    }
    let cd_offset = out.len() as u32;
    let cd_size = central.len() as u32;
    out.extend(central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    le16(&mut out, 0);
    le16(&mut out, 0);
    le16(&mut out, files.len() as u16);
    le16(&mut out, files.len() as u16);
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    le16(&mut out, 0);
    out
}

/// The area of interest as a zipped shapefile named `name` (.shp/.shx/.dbf/.prj).
pub fn aoi_zip(name: &str, ring: &[LonLat]) -> Vec<u8> {
    let (shp, shx) = shp_shx(ring);
    let dbf = dbf();
    let names = ["shp", "shx", "dbf", "prj"].map(|e| format!("{name}.{e}"));
    zip_store(&[(&names[0], &shp), (&names[1], &shx), (&names[2], &dbf), (&names[3], PRJ_WGS84.as_bytes())])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn shapefile_layout() {
        // Counter-clockwise input is written clockwise and closed.
        let ring = [[-93.62, 42.03], [-93.615, 42.03], [-93.615, 42.034], [-93.62, 42.034]];
        let (shp, shx) = shp_shx(&ring);
        assert_eq!(&shp[0..4], &9994i32.to_be_bytes());
        // File length in words matches the byte length.
        assert_eq!(i32::from_be_bytes(shp[24..28].try_into().unwrap()) * 2, shp.len() as i32);
        assert_eq!(i32::from_be_bytes(shx[24..28].try_into().unwrap()) * 2, shx.len() as i32);
        assert_eq!(shx.len(), 108);
        // 5 points (closed); the second point is the last corner (reversed order).
        let n = i32::from_le_bytes(shp[148..152].try_into().unwrap());
        assert_eq!(n, 5);
        let x = |k: usize| f64::from_le_bytes(shp[156 + 16 * k..164 + 16 * k].try_into().unwrap());
        let y = |k: usize| f64::from_le_bytes(shp[164 + 16 * k..172 + 16 * k].try_into().unwrap());
        // Clockwise from the north-west corner: north-east comes next.
        assert_eq!((x(0), y(0)), (-93.62, 42.034));
        assert_eq!((x(1), y(1)), (-93.615, 42.034));
        assert_eq!((x(4), y(4)), (x(0), y(0)));
        let d = dbf();
        assert_eq!(d.len(), 65 + 11 + 1);
    }

    #[test]
    fn zip_directory() {
        let z = aoi_zip("aoi", &[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]]);
        assert_eq!(&z[0..4], &[0x50, 0x4b, 0x03, 0x04]);
        let end = z.len() - 22;
        assert_eq!(&z[end..end + 4], &[0x50, 0x4b, 0x05, 0x06]);
        assert_eq!(u16::from_le_bytes([z[end + 10], z[end + 11]]), 4);
        // The first entry is aoi.shp with the right CRC.
        let len = u32::from_le_bytes(z[22..26].try_into().unwrap()) as usize;
        let name_len = u16::from_le_bytes([z[26], z[27]]) as usize;
        assert_eq!(&z[30..30 + name_len], b"aoi.shp");
        let data = &z[30 + name_len..30 + name_len + len];
        assert_eq!(u32::from_le_bytes(z[14..18].try_into().unwrap()), crc32(data));
    }
}
