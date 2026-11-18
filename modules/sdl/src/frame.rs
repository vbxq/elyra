pub struct Screen {
    pub width: usize,
    pub height: usize,
}

impl Screen {
    pub fn new(width: i64, height: i64) -> Result<Self, String> {
        if !(1..=4096).contains(&width) || !(1..=4096).contains(&height) {
            return Err(format!("size {width}x{height} is outside 1..=4096"));
        }
        Ok(Self {
            width: width as usize,
            height: height as usize,
        })
    }

    pub fn pixel_count(&self) -> usize {
        self.width * self.height
    }

    pub fn argb_len(&self) -> usize {
        self.pixel_count() * 4
    }

    pub fn check_argb(&self, frame: &[u8]) -> Result<(), String> {
        if frame.len() != self.argb_len() {
            return Err(format!(
                "the ARGB frame holds {} bytes, a {}x{} screen needs {}",
                frame.len(),
                self.width,
                self.height,
                self.argb_len()
            ));
        }
        Ok(())
    }

    pub fn check_point(&self, x: i64, y: i64) -> Result<(usize, usize), String> {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
            return Err(format!(
                "pixel ({x}, {y}) is outside the {}x{} screen",
                self.width, self.height
            ));
        }
        Ok((x as usize, y as usize))
    }

    pub fn indexed_to_argb(
        &self,
        pixels: &[u8],
        palette: &[u8],
        out: &mut [u8],
    ) -> Result<(), String> {
        if pixels.len() != self.pixel_count() {
            return Err(format!(
                "the indexed frame holds {} bytes, a {}x{} screen needs {}",
                pixels.len(),
                self.width,
                self.height,
                self.pixel_count()
            ));
        }
        if palette.len() != 768 {
            return Err(format!(
                "the palette holds {} bytes, 256 RGB entries need 768",
                palette.len()
            ));
        }
        let mut lookup = [0u32; 256];
        for (entry, rgb) in lookup.iter_mut().zip(palette.chunks_exact(3)) {
            *entry = 0xFF00_0000
                | (u32::from(rgb[0]) << 16)
                | (u32::from(rgb[1]) << 8)
                | u32::from(rgb[2]);
        }
        for (target, &index) in out.chunks_exact_mut(4).zip(pixels) {
            target.copy_from_slice(&lookup[usize::from(index)].to_le_bytes());
        }
        Ok(())
    }
}

pub fn counter_to_ns(ticks: u64, frequency: u64) -> i64 {
    if frequency == 0 {
        return 0;
    }
    let nanoseconds = u128::from(ticks) * 1_000_000_000 / u128::from(frequency);
    i64::try_from(nanoseconds).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexed_pixels_take_their_palette_colour_in_argb_byte_order() {
        let screen = Screen::new(3, 1).unwrap();
        let mut palette = vec![0u8; 768];
        palette[3..6].copy_from_slice(&[0x11, 0x22, 0x33]);
        palette[255 * 3..].copy_from_slice(&[0xAA, 0xBB, 0xCC]);
        let mut out = vec![0u8; screen.argb_len()];
        screen
            .indexed_to_argb(&[1, 255, 0], &palette, &mut out)
            .unwrap();
        assert_eq!(
            out,
            [
                0x33, 0x22, 0x11, 0xFF, 0xCC, 0xBB, 0xAA, 0xFF, 0, 0, 0, 0xFF
            ]
        );
    }

    #[test]
    fn wrong_sized_frames_and_palettes_are_refused() {
        let screen = Screen::new(320, 200).unwrap();
        let mut out = vec![0u8; screen.argb_len()];
        assert!(
            screen
                .indexed_to_argb(&[0; 63_999], &[0; 768], &mut out)
                .is_err()
        );
        assert!(
            screen
                .indexed_to_argb(&[0; 64_000], &[0; 767], &mut out)
                .is_err()
        );
        assert!(
            screen
                .indexed_to_argb(&[0; 64_001], &[0; 768], &mut out)
                .is_err()
        );
        assert!(
            screen
                .indexed_to_argb(&[0; 64_000], &[0; 769], &mut out)
                .is_err()
        );
        assert!(screen.check_argb(&[0; 255_999]).is_err());
        assert!(screen.check_argb(&[0; 256_004]).is_err());
        assert!(screen.check_argb(&[0; 256_000]).is_ok());
        assert!(Screen::new(0, 200).is_err());
        assert!(screen.check_point(320, 0).is_err());
        assert_eq!(screen.check_point(319, 199), Ok((319, 199)));
    }

    #[test]
    fn counter_ticks_convert_to_nanoseconds() {
        assert_eq!(counter_to_ns(3, 1_000), 3_000_000);
        assert_eq!(counter_to_ns(1_000_000_000, 1_000_000_000), 1_000_000_000);
        assert_eq!(counter_to_ns(5, 0), 0);
    }
}
