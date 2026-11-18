use super::{Result, invalid};

/// 有界 PE32+ 解析：不执行文件，不把字符串搜索当作导出/架构验证。
pub(super) fn validate(bytes: &[u8]) -> Result<()> {
    let bad =
        || invalid("登录入口必须是提供 SDOLGetModule/SDOLInitialize/SDOLTerminal 的 x64 PE DLL");
    let u16_at = |o: usize| -> Result<u16> {
        Ok(u16::from_le_bytes(
            bytes
                .get(o..o.checked_add(2).ok_or_else(bad)?)
                .ok_or_else(bad)?
                .try_into()
                .unwrap(),
        ))
    };
    let u32_at = |o: usize| -> Result<u32> {
        Ok(u32::from_le_bytes(
            bytes
                .get(o..o.checked_add(4).ok_or_else(bad)?)
                .ok_or_else(bad)?
                .try_into()
                .unwrap(),
        ))
    };
    if bytes.get(..2) != Some(b"MZ") {
        return Err(bad());
    }
    let pe = u32_at(0x3c)? as usize;
    if pe > bytes.len().saturating_sub(264)
        || bytes.get(pe..pe + 4) != Some(b"PE\0\0")
        || u16_at(pe + 4)? != 0x8664
        || u16_at(pe + 22)? & 0x2002 != 0x2002
        || u16_at(pe + 24)? != 0x20b
        || u16_at(pe + 20)? < 120
    {
        return Err(bad());
    }
    let count = u16_at(pe + 6)? as usize;
    if count == 0 || count > 96 {
        return Err(bad());
    }
    let sections = pe + 24 + u16_at(pe + 20)? as usize;
    if sections.saturating_add(count * 40) > bytes.len() {
        return Err(bad());
    }
    let offset = |rva: u32, size: usize| -> Result<usize> {
        for i in 0..count {
            let s = sections + i * 40;
            let va = u32_at(s + 12)?;
            let raw_size = u32_at(s + 16)? as usize;
            if rva < va {
                continue;
            }
            let delta = (rva - va) as usize;
            if delta.saturating_add(size) > raw_size {
                continue;
            }
            let at = (u32_at(s + 20)? as usize)
                .checked_add(delta)
                .ok_or_else(bad)?;
            if at.checked_add(size).is_some_and(|end| end <= bytes.len()) {
                return Ok(at);
            }
        }
        Err(bad())
    };
    if u32_at(pe + 24 + 108)? < 1 {
        return Err(bad());
    }
    let export_rva = u32_at(pe + 24 + 112)?;
    let export_size = u32_at(pe + 24 + 116)?;
    if export_rva == 0 || export_size < 40 {
        return Err(bad());
    }
    let export = offset(export_rva, 40)?;
    let functions = u32_at(export + 20)? as usize;
    let names = u32_at(export + 24)? as usize;
    if functions == 0 || functions > 4096 || names > 4096 {
        return Err(bad());
    }
    let funcs = offset(u32_at(export + 28)?, functions * 4)?;
    let table = offset(u32_at(export + 32)?, names * 4)?;
    let ords = offset(u32_at(export + 36)?, names * 2)?;
    let mut required = [false; 3];
    for i in 0..names {
        let name = offset(u32_at(table + i * 4)?, 1)?;
        let end = bytes[name..]
            .iter()
            .take(128)
            .position(|b| *b == 0)
            .ok_or_else(bad)?;
        let Some(index) = [
            b"SDOLGetModule".as_slice(),
            b"SDOLInitialize",
            b"SDOLTerminal",
        ]
        .iter()
        .position(|s| *s == &bytes[name..name + end]) else {
            continue;
        };
        let ordinal = u16_at(ords + i * 2)? as usize;
        if ordinal >= functions || required[index] {
            return Err(bad());
        }
        let target = u32_at(funcs + ordinal * 4)?;
        // 不接受需要额外 DLL 的导出转发器。
        if target == 0
            || (target >= export_rva
                && u64::from(target) < u64::from(export_rva) + u64::from(export_size))
        {
            return Err(bad());
        }
        offset(target, 1)?;
        let mut executable = false;
        for section in 0..count {
            let section = sections + section * 40;
            let va = u32_at(section + 12)?;
            if target >= va
                && target - va < u32_at(section + 16)?
                && u32_at(section + 36)? & 0x20000000 != 0
            {
                executable = true;
            }
        }
        if !executable {
            return Err(bad());
        }
        required[index] = true;
    }
    if required != [true; 3] {
        return Err(bad());
    }
    Ok(())
}
