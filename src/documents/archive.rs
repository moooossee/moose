use crate::i18n::gettext;

use super::MAX_FILE_BYTES;

pub struct Archive {
    data: Vec<u8>,
    entries: Vec<Vec<u8>>,
}

impl Archive {
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
            entries: Vec::new(),
        }
    }

    pub fn add(&mut self, name: &str, content: impl AsRef<[u8]>) -> Result<(), String> {
        let content = content.as_ref();
        if name.len() > u16::MAX as usize
            || self
                .data
                .len()
                .saturating_add(content.len())
                .saturating_add(name.len() * 2 + 128)
                > MAX_FILE_BYTES
        {
            return Err(gettext("The document is too large to package."));
        }
        let offset = self.data.len() as u32;
        let size = content.len() as u32;
        let crc = crc32(content);
        let mut header = Vec::new();
        u32_bytes(&mut header, 0x04034b50);
        for value in [20, 0x0800, 0, 0, 33] {
            u16_bytes(&mut header, value);
        }
        for value in [crc, size, size] {
            u32_bytes(&mut header, value);
        }
        u16_bytes(&mut header, name.len() as u16);
        u16_bytes(&mut header, 0);
        self.data.extend(header);
        self.data.extend(name.as_bytes());
        self.data.extend(content);
        let mut entry = Vec::new();
        u32_bytes(&mut entry, 0x02014b50);
        for value in [20, 20, 0x0800, 0, 0, 33] {
            u16_bytes(&mut entry, value);
        }
        for value in [crc, size, size] {
            u32_bytes(&mut entry, value);
        }
        for value in [name.len() as u16, 0, 0, 0, 0] {
            u16_bytes(&mut entry, value);
        }
        u32_bytes(&mut entry, 0);
        u32_bytes(&mut entry, offset);
        entry.extend(name.as_bytes());
        self.entries.push(entry);
        Ok(())
    }

    pub fn finish(mut self) -> Result<Vec<u8>, String> {
        if self.entries.len() > u16::MAX as usize {
            return Err(gettext("The document contains too many parts."));
        }
        let offset = self.data.len() as u32;
        let count = self.entries.len() as u16;
        for entry in self.entries {
            self.data.extend(entry);
        }
        let size = self.data.len() as u32 - offset;
        u32_bytes(&mut self.data, 0x06054b50);
        for value in [0, 0, count, count] {
            u16_bytes(&mut self.data, value);
        }
        u32_bytes(&mut self.data, size);
        u32_bytes(&mut self.data, offset);
        u16_bytes(&mut self.data, 0);
        if self.data.len() > MAX_FILE_BYTES {
            return Err(gettext("The generated document exceeds the size limit."));
        }
        Ok(self.data)
    }
}

fn u16_bytes(output: &mut Vec<u8>, value: u16) {
    output.extend(value.to_le_bytes());
}
fn u32_bytes(output: &mut Vec<u8>, value: u32) {
    output.extend(value.to_le_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}
