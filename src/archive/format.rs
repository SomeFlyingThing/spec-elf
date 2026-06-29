use crate::arch::x86::{X86Level, detect_x86_level, native_hasher};
use std::{
    fs::{File, OpenOptions},
    io::{self, Error, ErrorKind, Read, Seek, SeekFrom, Write},
    path::Path,
};

const FOOTER_MAGIC: &[u8; 8] = b"VPKFOOT\0";
const FOOTER_SIZE: u64 = 33;
const IS_LAUNCHED: u8 = 1; // use to be written in the launcher, must be 1

/// A single packed payload entry stored in the manifest.
struct Entry {
    name: String,
    offset: u64,
    size: u64,
}

/// Read a little-endian `u32` from the current file position.
fn read_u32(file: &mut File) -> io::Result<u32> {
    let mut buf = [0u8; 4];
    file.read_exact(&mut buf)?;
    Ok(u32::from_le_bytes(buf))
}

/// Read a little-endian `u64` from the current file position.
fn read_u64(file: &mut File) -> io::Result<u64> {
    let mut buf = [0u8; 8];
    file.read_exact(&mut buf)?;
    Ok(u64::from_le_bytes(buf))
}

/// Build a packed executable by appending payloads and a manifest footer.
pub fn pack_files<P, O>(launcher_path: P, output_path: O, payload_paths: &[String]) -> io::Result<()>
where
    P: AsRef<Path>,
    O: AsRef<Path>,
{
    let mut output = OpenOptions::new().create(true).truncate(true).write(true).open(output_path)?;

    let mut launcher = File::open(launcher_path)?;
    io::copy(&mut launcher, &mut output)?;

    let mut entries = Vec::with_capacity(payload_paths.len());

    for payload_path in payload_paths {
        let payload_path = Path::new(payload_path);
        let offset = output.stream_position()?;
        let mut payload = File::open(payload_path).map_err(|err| Error::new(err.kind(), format!("failed to open payload {}: {err}", payload_path.display())))?;
        let size = io::copy(&mut payload, &mut output)?;
        let name = payload_path.file_name().and_then(|name| name.to_str()).ok_or_else(|| Error::new(ErrorKind::InvalidInput, "payload path has no valid file name"))?.to_string();

        entries.push(Entry { name, offset, size });
    }

    // The manifest is appended after the launcher + payload blobs.
    let manifest_offset = output.stream_position()?;
    output.write_all(&(entries.len() as u32).to_le_bytes())?;

    for entry in &entries {
        let name_bytes = entry.name.as_bytes();
        output.write_all(&(name_bytes.len() as u32).to_le_bytes())?;
        output.write_all(name_bytes)?;
        output.write_all(&entry.offset.to_le_bytes())?;
        output.write_all(&entry.size.to_le_bytes())?;
    }

    let manifest_size = output.stream_position()? - manifest_offset;
    output.write_all(FOOTER_MAGIC)?;
    output.write_all(&manifest_offset.to_le_bytes())?;
    output.write_all(&manifest_size.to_le_bytes())?;

    let native_hash = native_hasher();

    match native_hash {
        Some(v) => output.write_all(&v.to_le_bytes())?,
        None => output.write_all(&((0 as u64).to_le_bytes()))?,
    }

    // Reserved flag for launch bookkeeping.
    // is launched needs to be 1
    output.write_all(&[IS_LAUNCHED])?;

    Ok(())
}

/// Read the packed file footer, locate the best matching payload, and return it
pub fn read_back<P>(path: P) -> io::Result<Vec<u8>>
where
    P: AsRef<Path>,
{
    let mut file = OpenOptions::new().read(true).open(path)?;

    let file_size = file.metadata()?.len();

    if file_size < FOOTER_SIZE {
        return Err(Error::new(ErrorKind::InvalidData, "file too small"));
    }

    file.seek(SeekFrom::End(-(FOOTER_SIZE as i64)))?;

    let mut magic = [0u8; 8];
    file.read_exact(&mut magic)?;

    if &magic != FOOTER_MAGIC {
        return Err(Error::new(ErrorKind::InvalidData, "invalid footer magic"));
    }

    // Footer layout: magic, manifest offset, manifest size, native hash, launch flag.
    let manifest_offset = read_u64(&mut file)?;
    let manifest_size = read_u64(&mut file)?;

    if manifest_offset + manifest_size > file_size - FOOTER_SIZE {
        return Err(Error::new(ErrorKind::InvalidData, "invalid manifest range"));
    }

    file.seek(SeekFrom::Start(manifest_offset))?;

    let entry_count = read_u32(&mut file)?;

    // Each manifest entry stores the payload name and its byte range.
    let mut entries = Vec::with_capacity(entry_count as usize);

    for _ in 0..entry_count {
        let name_len = read_u32(&mut file)? as usize;

        let mut name_bytes = vec![0u8; name_len];
        file.read_exact(&mut name_bytes)?;

        let name = String::from_utf8(name_bytes).map_err(|_| Error::new(ErrorKind::InvalidData, "invalid UTF-8 in file name"))?;

        let offset = read_u64(&mut file)?;
        let size = read_u64(&mut file)?;

        entries.push(Entry { name, offset, size });
    }

    file.seek(SeekFrom::End(-9))?;
    let mut native_hash = [0u8; 8];
    file.read_exact(&mut native_hash)?;

    let (offset, size) = find_optimal(&entries, &native_hash)?;

    let mut correct_exe = vec![0u8; size as usize];

    file.seek(SeekFrom::Start(offset))?;
    file.read_exact(&mut correct_exe)?;

    Ok(correct_exe)
}

/// Pick the payload that best matches the CPU's supported x86-64 level.
fn find_optimal(entries: &[Entry], native_hash: &[u8]) -> io::Result<(u64, u64)> {
    let level = detect_x86_level();

    // if the hashes are equal we itenerate and see if one contains native as part of name, if yes we return the offset and size if this is not true we just continue default path
    if let Some(b) = native_hasher() {
        //return the native as a choice
        if b.to_le_bytes() == native_hash {
            for entry in entries {
                if entry.name.contains("native") {
                    return Ok((entry.offset, entry.size));
                }
            }
        }
    }

    let wanted = match level {
        X86Level::V4 => "x86-64-v4",
        X86Level::V3 => "x86-64-v3",
        X86Level::V2 => "x86-64-v2",
        X86Level::X86_64 => "x86-64",
    };
    let wanted_with_underscores = wanted.replace('-', "_");

    for entry in entries {
        if entry.name == wanted || entry.name.ends_with(wanted) || entry.name == wanted_with_underscores || entry.name.ends_with(&wanted_with_underscores) || entry.name == format!("-march={wanted}") {
            return Ok((entry.offset, entry.size));
        }
    }

    Err(io::Error::new(io::ErrorKind::NotFound, "no compatible binary found"))
}

pub fn is_archive<P>(path: P) -> io::Result<bool>
where
    P: AsRef<Path>,
{
    let mut file = OpenOptions::new().read(true).open(path)?;

    let file_size = file.metadata()?.len();

    if file_size < FOOTER_SIZE {
        return Ok(false);
    }

    file.seek(SeekFrom::End(-(FOOTER_SIZE as i64)))?;

    let mut identifier = [0u8; 8];
    file.read_exact(&mut identifier)?;

    //check the last byte bcs it is a u8 FOOTER_IS_LAUNCHED
    if &identifier == FOOTER_MAGIC {
        file.seek(SeekFrom::End(-1))?;
        let mut is_launched = [0u8; 1];
        file.read_exact(&mut is_launched)?;

        if is_launched[0] == 1 {
            return Ok(true);
        }
    }

    Ok(false)
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::arch::x86::{X86Level, detect_x86_level, native_hasher};
    use std::fs;

    #[test]
    fn normal_file_is_not_archive() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("plain-file");

        fs::write(&file, b"hello")?;

        assert!(!is_archive(&file)?);

        Ok(())
    }

    #[test]
    fn packed_file_is_archive() -> io::Result<()> {
        let dir = tempfile::tempdir()?;

        let launcher = dir.path().join("launcher");
        let output = dir.path().join("packed");

        let native = dir.path().join("c-native");
        let x86_64 = dir.path().join("c-x86-64");
        let v2 = dir.path().join("c-x86-64-v2");
        let v3 = dir.path().join("c-x86-64-v3");
        let v4 = dir.path().join("c-x86-64-v4");

        fs::write(&launcher, b"fake launcher")?;
        fs::write(&native, b"native payload")?;
        fs::write(&x86_64, b"x86-64 payload")?;
        fs::write(&v2, b"x86-64-v2 payload")?;
        fs::write(&v3, b"x86-64-v3 payload")?;
        fs::write(&v4, b"x86-64-v4 payload")?;

        let payloads = vec![
            native.display().to_string(),
            x86_64.display().to_string(),
            v2.display().to_string(),
            v3.display().to_string(),
            v4.display().to_string(),
        ];

        pack_files(&launcher, &output, &payloads)?;

        assert!(is_archive(&output)?);

        Ok(())
    }

    #[test]
    fn packed_file_reads_best_payload() -> io::Result<()> {
        let dir = tempfile::tempdir()?;

        let launcher = dir.path().join("launcher");
        let output = dir.path().join("packed");

        let native = dir.path().join("c-native");
        let x86_64 = dir.path().join("c-x86-64");
        let v2 = dir.path().join("c-x86-64-v2");
        let v3 = dir.path().join("c-x86-64-v3");
        let v4 = dir.path().join("c-x86-64-v4");

        fs::write(&launcher, b"fake launcher")?;
        fs::write(&native, b"native payload")?;
        fs::write(&x86_64, b"x86-64 payload")?;
        fs::write(&v2, b"x86-64-v2 payload")?;
        fs::write(&v3, b"x86-64-v3 payload")?;
        fs::write(&v4, b"x86-64-v4 payload")?;

        let payloads = vec![
            native.display().to_string(),
            x86_64.display().to_string(),
            v2.display().to_string(),
            v3.display().to_string(),
            v4.display().to_string(),
        ];

        pack_files(&launcher, &output, &payloads)?;

        let actual = read_back(&output)?;

        let expected: &[u8] = if native_hasher().is_some() {
            b"native payload"
        } else {
            match detect_x86_level() {
                X86Level::X86_64 => b"x86-64 payload",
                X86Level::V2 => b"x86-64-v2 payload",
                X86Level::V3 => b"x86-64-v3 payload",
                X86Level::V4 => b"x86-64-v4 payload",
            }
        };

        assert_eq!(actual, expected);

        Ok(())
    }
}