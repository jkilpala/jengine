//! Reading a Photoshop document's layers (`.psd`, version 1): each pixel
//! layer's bounds, pixels, opacity, visibility, blend mode and the groups it
//! sits in - what [`crate::psd_import`] turns into sprites and a prefab.
//!
//! What it reads: 8-bit RGB and greyscale documents, channels stored raw
//! or RLE (PackBits) - which is what Photoshop writes for 8-bit layers.
//! What it refuses, by name rather than by misreading: large documents
//! (`.psb`), 16 and 32-bit depth, other colour modes (CMYK, indexed, ...)
//! and zip-compressed channels. A layer's **mask** is applied to its alpha
//! (some writers, psd-tools among them, keep a layer's transparency there);
//! adjustment layers' settings, effects, vector masks and text are not read.
//!
//! Layers come out **bottom first**, the order they draw in. A group is
//! read from its section markers (`lsct`): its children carry its name in
//! [`PsdLayer::groups`], outermost first, and a hidden group hides them.

use anyhow::{Context, Result, anyhow, bail};

/// A layered document: its canvas, and its pixel layers bottom first.
#[derive(Debug, Clone)]
pub struct PsdDocument {
    pub width: u32,
    pub height: u32,
    pub layers: Vec<PsdLayer>,
}

/// One pixel layer.
#[derive(Debug, Clone)]
pub struct PsdLayer {
    pub name: String,
    /// The groups it is in, outermost first.
    pub groups: Vec<String>,
    /// Its bounds on the canvas, in pixels from the top-left.
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    /// Its pixels, RGBA, row by row from the top.
    pub rgba: Vec<u8>,
    /// 0 to 255.
    pub opacity: u8,
    /// Its own eye and every group's above it.
    pub visible: bool,
    /// Photoshop's four-letter blend key: `norm`, `mul `, `scrn`, `lddg`
    /// (linear dodge, add), ...
    pub blend: String,
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.at.checked_add(n).filter(|&e| e <= self.data.len());
        let end = end.ok_or_else(|| anyhow!("the file ends early (wanted {n} bytes at {})", self.at))?;
        let out = &self.data[self.at..end];
        self.at = end;
        Ok(out)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }
    fn i16(&mut self) -> Result<i16> {
        Ok(self.u16()? as i16)
    }
    fn u32(&mut self) -> Result<u32> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn i32(&mut self) -> Result<i32> {
        Ok(self.u32()? as i32)
    }
    fn skip_block(&mut self) -> Result<()> {
        let n = self.u32()? as usize;
        self.take(n)?;
        Ok(())
    }
}

/// One layer record, before its pixels are read.
struct Record {
    name: String,
    top: i32,
    left: i32,
    bottom: i32,
    right: i32,
    /// (channel id, byte length)
    channels: Vec<(i16, u32)>,
    blend: String,
    opacity: u8,
    hidden: bool,
    /// `lsct`: 1 or 2 opens a group (its record is the group), 3 closes one.
    section: u32,
    mask: Option<Mask>,
}

/// A layer mask: its own rectangle on the canvas and the value outside it.
#[derive(Clone, Copy)]
struct Mask {
    top: i32,
    left: i32,
    bottom: i32,
    right: i32,
    outside: u8,
}

/// Read a `.psd`'s layers.
pub fn read_psd(data: &[u8]) -> Result<PsdDocument> {
    let mut r = Reader { data, at: 0 };
    if r.take(4)? != b"8BPS" {
        bail!("not a Photoshop document (no 8BPS signature)");
    }
    match r.u16()? {
        1 => {}
        2 => bail!("a large document (.psb) - save it as a .psd"),
        v => bail!("unknown PSD version {v}"),
    }
    r.take(6)?;
    let _channels = r.u16()?;
    let height = r.u32()?;
    let width = r.u32()?;
    let depth = r.u16()?;
    if depth != 8 {
        bail!("{depth}-bit channels - only 8-bit documents are read (Image > Mode > 8 Bits/Channel)");
    }
    let mode = r.u16()?;
    let grey = match mode {
        3 => false,
        1 => true,
        4 => bail!("a CMYK document - convert it to RGB (Image > Mode > RGB Color)"),
        m => bail!("colour mode {m} - only RGB and greyscale documents are read"),
    };
    r.skip_block().context("colour mode data")?;
    r.skip_block().context("image resources")?;

    let layer_and_mask = r.u32()? as usize;
    if layer_and_mask == 0 {
        return Ok(PsdDocument { width, height, layers: Vec::new() });
    }
    let info_len = r.u32()? as usize;
    if info_len == 0 {
        return Ok(PsdDocument { width, height, layers: Vec::new() });
    }
    let info_end = r.at + info_len;
    let count = r.i16()?.unsigned_abs() as usize;

    let mut records = Vec::with_capacity(count);
    for i in 0..count {
        records.push(read_record(&mut r).with_context(|| format!("layer record {i}"))?);
    }

    // Pixels follow, layer by layer, channel by channel.
    let mut pixels = Vec::with_capacity(count);
    for (i, rec) in records.iter().enumerate() {
        let w = (rec.right - rec.left).max(0) as usize;
        let h = (rec.bottom - rec.top).max(0) as usize;
        let mut rgba = vec![0u8; w * h * 4];
        let mut has_alpha = false;
        let mut mask: Option<Vec<u8>> = None;
        for &(id, len) in &rec.channels {
            let block = r.take(len as usize).with_context(|| format!("layer {i} ({}) channel {id}", rec.name))?;
            let target = match (id, grey) {
                (0, false) => Some(vec![0]),
                (1, false) => Some(vec![1]),
                (2, false) => Some(vec![2]),
                (0, true) => Some(vec![0, 1, 2]),
                (-1, _) => Some(vec![3]),
                // The layer mask: sized by its own rectangle.
                (-2, _) => {
                    if let Some(m) = rec.mask {
                        let (mw, mh) = ((m.right - m.left).max(0) as usize, (m.bottom - m.top).max(0) as usize);
                        if mw > 0 && mh > 0 {
                            mask = Some(
                                decode_channel(block, mw, mh)
                                    .with_context(|| format!("layer {i} ({}) mask", rec.name))?,
                            );
                        }
                    }
                    None
                }
                // A vector mask's pixels and anything else: not read.
                _ => None,
            };
            let Some(target) = target else { continue };
            if id == -1 {
                has_alpha = true;
            }
            if w == 0 || h == 0 {
                continue;
            }
            let plane = decode_channel(block, w, h).with_context(|| format!("layer {i} ({}) channel {id}", rec.name))?;
            for (p, v) in plane.iter().enumerate() {
                for &c in &target {
                    rgba[p * 4 + c] = *v;
                }
            }
        }
        if !has_alpha {
            for p in 0..w * h {
                rgba[p * 4 + 3] = 255;
            }
        }
        // The mask scales the alpha: its value inside its rectangle, its
        // default outside.
        if let Some(m) = rec.mask {
            let mw = (m.right - m.left).max(0) as usize;
            for y in 0..h {
                for x in 0..w {
                    let (cx, cy) = (rec.left + x as i32, rec.top + y as i32);
                    let inside = cx >= m.left && cx < m.right && cy >= m.top && cy < m.bottom;
                    let v = match (&mask, inside) {
                        (Some(px), true) => px[(cy - m.top) as usize * mw + (cx - m.left) as usize],
                        _ => m.outside,
                    };
                    let a = &mut rgba[(y * w + x) * 4 + 3];
                    *a = ((*a as u32 * v as u32 + 127) / 255) as u8;
                }
            }
        }
        pixels.push(rgba);
    }
    if r.at > info_end {
        bail!("the layer data runs past its block");
    }

    // Records run bottom first; a group's closing marker comes below its
    // children and its own record above them. Walk from the top so a
    // group's name is known before its children are met.
    let mut layers = Vec::new();
    let mut stack: Vec<(String, bool)> = Vec::new();
    for (rec, rgba) in records.into_iter().zip(pixels).rev() {
        match rec.section {
            1 | 2 => {
                stack.push((rec.name, !rec.hidden));
                continue;
            }
            3 => {
                stack.pop();
                continue;
            }
            _ => {}
        }
        let width = (rec.right - rec.left).max(0) as u32;
        let height = (rec.bottom - rec.top).max(0) as u32;
        layers.push(PsdLayer {
            name: rec.name,
            groups: stack.iter().map(|(n, _)| n.clone()).collect(),
            left: rec.left,
            top: rec.top,
            width,
            height,
            rgba,
            opacity: rec.opacity,
            visible: !rec.hidden && stack.iter().all(|(_, v)| *v),
            blend: rec.blend,
        });
    }
    layers.reverse();
    Ok(PsdDocument { width, height, layers })
}

fn read_record(r: &mut Reader) -> Result<Record> {
    let top = r.i32()?;
    let left = r.i32()?;
    let bottom = r.i32()?;
    let right = r.i32()?;
    let n = r.u16()? as usize;
    let mut channels = Vec::with_capacity(n);
    for _ in 0..n {
        let id = r.i16()?;
        let len = r.u32()?;
        channels.push((id, len));
    }
    if r.take(4)? != b"8BIM" {
        bail!("no blend mode signature");
    }
    let blend = String::from_utf8_lossy(r.take(4)?).into_owned();
    let opacity = r.u8()?;
    let _clipping = r.u8()?;
    let flags = r.u8()?;
    let _filler = r.u8()?;
    let extra_len = r.u32()? as usize;
    let extra_end = r.at + extra_len;
    // The layer mask's rectangle, default and flags (bit 1: disabled).
    let mask_len = r.u32()? as usize;
    let mask_block = r.take(mask_len).context("layer mask")?;
    let mask = if mask_len >= 18 {
        let mut m = Reader { data: mask_block, at: 0 };
        let (top, left, bottom, right) = (m.i32()?, m.i32()?, m.i32()?, m.i32()?);
        let outside = m.u8()?;
        let flags = m.u8()?;
        (flags & 0x02 == 0).then_some(Mask { top, left, bottom, right, outside })
    } else {
        None
    };
    r.skip_block().context("blending ranges")?;
    // A Pascal name, padded to a multiple of four bytes with its length.
    let name_len = r.u8()? as usize;
    let mut name = String::from_utf8_lossy(r.take(name_len)?).into_owned();
    let used = 1 + name_len;
    r.take((4 - used % 4) % 4)?;
    let mut section = 0;
    // Additional layer information, up to the end of the record.
    while r.at + 12 <= extra_end {
        let sig = r.take(4)?;
        if sig != b"8BIM" && sig != b"8B64" {
            bail!("an unknown block signature in the layer's extra data");
        }
        let key = r.take(4)?;
        let len = r.u32()? as usize;
        let body = r.take(len)?;
        let mut b = Reader { data: body, at: 0 };
        match key {
            // The name in full (the Pascal one is cut short and not Unicode).
            b"luni" => {
                let chars = b.u32()? as usize;
                let units: Vec<u16> = (0..chars).map(|_| b.u16()).collect::<Result<_>>()?;
                name = String::from_utf16_lossy(&units).trim_end_matches('\0').to_string();
            }
            b"lsct" | b"lsdk" => section = b.u32()?,
            _ => {}
        }
    }
    if r.at > extra_end {
        bail!("the layer's extra data runs past its record");
    }
    r.at = extra_end;
    Ok(Record { name, top, left, bottom, right, channels, blend, opacity, hidden: flags & 0x02 != 0, section, mask })
}

/// One channel's `w` x `h` bytes from its block: a compression word, then
/// raw rows or PackBits rows after a table of their lengths.
fn decode_channel(block: &[u8], w: usize, h: usize) -> Result<Vec<u8>> {
    let mut r = Reader { data: block, at: 0 };
    match r.u16()? {
        0 => Ok(r.take(w * h).context("raw pixels")?.to_vec()),
        1 => {
            let lengths: Vec<usize> = (0..h).map(|_| r.u16().map(|n| n as usize)).collect::<Result<_>>()?;
            let mut out = Vec::with_capacity(w * h);
            for len in lengths {
                let row = unpack_bits(r.take(len)?, w)?;
                out.extend_from_slice(&row);
            }
            Ok(out)
        }
        2 | 3 => bail!("zip-compressed channels - save with RLE (Photoshop does for 8-bit layers)"),
        c => bail!("unknown channel compression {c}"),
    }
}

/// PackBits: a count byte `n`; `0..=127` copies `n + 1` bytes, `-1..=-127`
/// repeats the next byte `1 - n` times, `-128` is nothing.
fn unpack_bits(data: &[u8], width: usize) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(width);
    let mut i = 0;
    while i < data.len() && out.len() < width {
        let n = data[i] as i8;
        i += 1;
        if n >= 0 {
            let k = n as usize + 1;
            let run = data.get(i..i + k).ok_or_else(|| anyhow!("a literal run past the row"))?;
            out.extend_from_slice(run);
            i += k;
        } else if n != -128 {
            let v = *data.get(i).ok_or_else(|| anyhow!("a repeat past the row"))?;
            out.extend(std::iter::repeat_n(v, (1 - n as i32) as usize));
            i += 1;
        }
    }
    if out.len() < width {
        bail!("a row of {} pixels where {width} were wanted", out.len());
    }
    out.truncate(width);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packbits_literal_and_repeat_runs() {
        // 2 literals, then 3 x 0xAA, then a no-op.
        assert_eq!(unpack_bits(&[1, 7, 8, 0xFE, 0xAA, 0x80], 5).unwrap(), vec![7, 8, 0xAA, 0xAA, 0xAA]);
        assert!(unpack_bits(&[0xFE, 1], 5).is_err());
    }

    #[test]
    fn refuses_what_it_cannot_read() {
        assert!(read_psd(b"GIF89a").unwrap_err().to_string().contains("not a Photoshop document"));
        let mut psb = b"8BPS".to_vec();
        psb.extend_from_slice(&2u16.to_be_bytes());
        assert!(read_psd(&psb).unwrap_err().to_string().contains(".psb"));
    }
}
