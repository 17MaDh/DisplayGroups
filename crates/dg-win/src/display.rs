//! `DisplayBackend` over CCD (`QueryDisplayConfig` / `SetDisplayConfig`), GDI mode
//! APIs and `DisplayConfig{Get,Set}DeviceInfo`.

use std::collections::HashSet;
use std::mem::size_of;
use std::ptr::{null, null_mut};

use dg_engine::ccd::{
    Luid, ModeInfo, PathInfo, PathSource, PathTarget, Rational, Scope, TargetDetails, TargetRef,
    Topology, VideoSignal,
};
use dg_engine::dpi::DpiInfo;
use dg_engine::{DeviceMode, DisplayBackend, EngineError, HdrState, ModeChange};

use crate::ffi::*;
use crate::util::{from_wide, wide};

/// The real display backend. Stateless: every call queries Windows afresh.
#[derive(Debug, Default)]
pub struct WinBackend;

impl WinBackend {
    pub fn new() -> Self {
        Self
    }
}

impl DisplayBackend for WinBackend {
    fn query(&self, scope: Scope) -> Result<Topology, EngineError> {
        let flags = match scope {
            Scope::All => QDC_ALL_PATHS,
            Scope::Active => QDC_ONLY_ACTIVE_PATHS,
        };
        let (raw_paths, raw_modes) = query_raw(flags)?;
        let paths: Vec<PathInfo> = raw_paths.iter().map(path_in).collect();
        let modes = raw_modes.iter().map(mode_in).collect();

        let mut targets = Vec::new();
        let mut seen = HashSet::new();
        for p in &paths {
            let t = p.target_ref();
            if !seen.insert(t) {
                continue;
            }
            let available = paths
                .iter()
                .any(|q| q.target_ref() == t && q.target.available);
            // QDC_ALL_PATHS also lists long-gone targets; skip their (slow) lookups.
            if scope == Scope::All && !available {
                continue;
            }
            targets.push(target_details(t, available, p.target.output_technology));
        }
        Ok(Topology {
            paths,
            modes,
            targets,
        })
    }

    fn set_config(&self, paths: &[PathInfo], modes: &[ModeInfo], flags: u32) -> i32 {
        let raw_paths: Vec<PATH_INFO> = paths.iter().map(path_out).collect();
        let raw_modes: Vec<MODE_INFO> = modes.iter().map(mode_out).collect();
        let (pp, np) = if raw_paths.is_empty() {
            (null(), 0)
        } else {
            (raw_paths.as_ptr(), len_u32(raw_paths.len()))
        };
        let (mp, nm) = if raw_modes.is_empty() {
            (null(), 0)
        } else {
            (raw_modes.as_ptr(), len_u32(raw_modes.len()))
        };
        // SAFETY: the pointers reference live, correctly laid-out arrays of the given lengths.
        let code = unsafe { SetDisplayConfig(np, pp, nm, mp, flags) };
        tracing::debug!(
            flags = format_args!("{flags:#06x}"),
            paths = np,
            modes = nm,
            code,
            "SetDisplayConfig"
        );
        code
    }

    fn list_modes(&self, target: TargetRef) -> Result<Vec<DeviceMode>, EngineError> {
        let name = gdi_name(target)?;
        let mut out = Vec::new();
        for i in 0..20_000u32 {
            let mut dm = devmode();
            // SAFETY: `name` is NUL-terminated and `dm.dmSize` is set.
            if unsafe { EnumDisplaySettingsExW(name.as_ptr(), i, &mut dm, 0) } == 0 {
                break;
            }
            let m = device_mode(&dm);
            if !out.contains(&m) {
                out.push(m);
            }
        }
        Ok(out)
    }

    fn current_mode(&self, target: TargetRef) -> Result<DeviceMode, EngineError> {
        let name = gdi_name(target)?;
        let mut dm = devmode();
        // SAFETY: as above.
        if unsafe { EnumDisplaySettingsExW(name.as_ptr(), ENUM_CURRENT_SETTINGS, &mut dm, 0) } == 0
        {
            return Err(EngineError::Win32 {
                op: "EnumDisplaySettingsExW",
                code: 0,
            });
        }
        Ok(device_mode(&dm))
    }

    fn apply_modes(&self, changes: &[ModeChange]) -> Result<Vec<i32>, EngineError> {
        let mut codes = Vec::with_capacity(changes.len());
        for c in changes {
            let Ok(name) = gdi_name(c.target) else {
                codes.push(-1);
                continue;
            };
            let mut dm = devmode();
            // SAFETY: as above.
            unsafe { EnumDisplaySettingsExW(name.as_ptr(), ENUM_CURRENT_SETTINGS, &mut dm, 0) };
            dm.dmPelsWidth = c.width;
            dm.dmPelsHeight = c.height;
            dm.dmDisplayFrequency = c.refresh_hz;
            dm.dmPositionX = c.x;
            dm.dmPositionY = c.y;
            dm.dmFields = DM_PELSWIDTH | DM_PELSHEIGHT | DM_DISPLAYFREQUENCY | DM_POSITION;
            if let Some(o) = c.orientation {
                dm.dmDisplayOrientation = o;
                dm.dmFields |= DM_DISPLAYORIENTATION;
            }
            let mut flags = CDS_UPDATEREGISTRY | CDS_NORESET;
            if c.primary {
                flags |= CDS_SET_PRIMARY;
            }
            // SAFETY: valid NUL-terminated device name and initialised DEVMODEW.
            codes.push(unsafe {
                ChangeDisplaySettingsExW(name.as_ptr(), &dm, null_mut(), flags, null())
            });
        }
        if codes.contains(&0) {
            // Commit the staged (CDS_NORESET) changes in one mode set.
            // SAFETY: all-null call is the documented "apply pending changes" form.
            unsafe { ChangeDisplaySettingsExW(null(), null(), null_mut(), 0, null()) };
        }
        Ok(codes)
    }

    fn get_dpi(&self, target: TargetRef) -> Result<DpiInfo, EngineError> {
        let (adapter, id) = source_of(target)?;
        let mut p = DPI_GET {
            header: header(GET_DPI_SCALE, size_of::<DPI_GET>(), adapter, id),
            ..Default::default()
        };
        device_info_get(&mut p.header, "DisplayConfigGetDeviceInfo(DPI)")?;
        Ok(DpiInfo {
            min_rel: p.minScaleRel,
            cur_rel: p.curScaleRel,
            max_rel: p.maxScaleRel,
        })
    }

    fn set_dpi(&self, target: TargetRef, relative: i32) -> Result<(), EngineError> {
        let (adapter, id) = source_of(target)?;
        let mut p = DPI_SET {
            header: header(SET_DPI_SCALE, size_of::<DPI_SET>(), adapter, id),
            scaleRel: relative,
        };
        device_info_set(&mut p.header, "DisplayConfigSetDeviceInfo(DPI)")
    }

    fn get_hdr(&self, target: TargetRef) -> Result<HdrState, EngineError> {
        hdr_state(target).map(|(state, _)| state)
    }

    fn set_hdr(&self, target: TargetRef, enabled: bool) -> Result<(), EngineError> {
        let (_, new_api) = hdr_state(target)?;
        let kind = if new_api {
            SET_HDR_STATE
        } else {
            SET_ADVANCED_COLOR_STATE
        };
        let mut p = SET_STATE {
            header: header(
                kind,
                size_of::<SET_STATE>(),
                luid_out(target.adapter),
                target.id,
            ),
            value: u32::from(enabled),
        };
        device_info_set(&mut p.header, "DisplayConfigSetDeviceInfo(HDR)")
    }

    fn sleep_ms(&self, ms: u32) {
        std::thread::sleep(std::time::Duration::from_millis(ms.into()));
    }
}

// ----- CCD queries -----

fn query_raw(flags: u32) -> Result<(Vec<PATH_INFO>, Vec<MODE_INFO>), EngineError> {
    // The topology can change between the size query and the data query; retry then.
    for _ in 0..8 {
        let (mut np, mut nm) = (0u32, 0u32);
        // SAFETY: out-pointers to locals.
        let r = unsafe { GetDisplayConfigBufferSizes(flags, &mut np, &mut nm) };
        if r != 0 {
            return Err(win_err("GetDisplayConfigBufferSizes", r));
        }
        let mut paths = vec![PATH_INFO::default(); np as usize];
        let mut modes = vec![MODE_INFO::default(); nm as usize];
        // SAFETY: buffers sized as Windows asked; topology id must be null for these flags.
        let r = unsafe {
            QueryDisplayConfig(
                flags,
                &mut np,
                paths.as_mut_ptr(),
                &mut nm,
                modes.as_mut_ptr(),
                null_mut(),
            )
        };
        if r == ERROR_INSUFFICIENT_BUFFER {
            continue;
        }
        if r != 0 {
            return Err(win_err("QueryDisplayConfig", r));
        }
        paths.truncate(np as usize);
        modes.truncate(nm as usize);
        return Ok((paths, modes));
    }
    Err(win_err("QueryDisplayConfig", ERROR_INSUFFICIENT_BUFFER))
}

fn win_err(op: &'static str, code: i32) -> EngineError {
    if code == dg_engine::ccd::win32::ACCESS_DENIED {
        EngineError::Locked
    } else {
        EngineError::Win32 { op, code }
    }
}

fn header(kind: i32, size: usize, adapter: LUID, id: u32) -> DEVICE_INFO_HEADER {
    DEVICE_INFO_HEADER {
        type_: kind,
        size: len_u32(size),
        adapterId: adapter,
        id,
    }
}

fn device_info_get(h: &mut DEVICE_INFO_HEADER, op: &'static str) -> Result<(), EngineError> {
    // SAFETY: `h` is the first field of a packet whose size is recorded in `h.size`.
    match unsafe { DisplayConfigGetDeviceInfo(h) } {
        0 => Ok(()),
        code => Err(win_err(op, code)),
    }
}

fn device_info_set(h: &mut DEVICE_INFO_HEADER, op: &'static str) -> Result<(), EngineError> {
    // SAFETY: as above.
    match unsafe { DisplayConfigSetDeviceInfo(h) } {
        0 => Ok(()),
        code => Err(win_err(op, code)),
    }
}

fn target_details(t: TargetRef, available: bool, output_technology: u32) -> TargetDetails {
    // SAFETY: plain-old-data packet; zero is a valid initial state.
    let mut n: TARGET_DEVICE_NAME = unsafe { std::mem::zeroed() };
    n.header = header(
        GET_TARGET_NAME,
        size_of::<TARGET_DEVICE_NAME>(),
        luid_out(t.adapter),
        t.id,
    );
    let ok = device_info_get(&mut n.header, "DisplayConfigGetDeviceInfo(TARGET_NAME)").is_ok();
    let edid_ids_valid = ok && n.flags & 0x4 != 0;
    let device_path = if ok {
        from_wide(&n.monitorDevicePath)
    } else {
        String::new()
    };
    let edid = if available && !device_path.is_empty() {
        read_edid(&device_path)
    } else {
        None
    };
    TargetDetails {
        target: t,
        available,
        friendly_name: if ok {
            from_wide(&n.monitorFriendlyDeviceName)
        } else {
            String::new()
        },
        edid_manufacture_id: edid_ids_valid.then_some(n.edidManufactureId),
        edid_product_code: edid_ids_valid.then_some(n.edidProductCodeId),
        connector_instance: if ok { n.connectorInstance } else { 0 },
        device_path,
        output_technology: if ok {
            n.outputTechnology
        } else {
            output_technology
        },
        edid,
    }
}

/// EDID bytes from `HKLM\SYSTEM\CurrentControlSet\Enum\<instance>\Device Parameters`.
fn read_edid(device_path: &str) -> Option<Vec<u8>> {
    let instance = dg_identity::instance_id_from_device_path(device_path)?;
    let subkey = wide(&format!(
        r"SYSTEM\CurrentControlSet\Enum\{instance}\Device Parameters"
    ));
    let value = wide("EDID");
    let mut size = 0u32;
    // SAFETY: size query with null data pointer.
    let r = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            subkey.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_BINARY,
            null_mut(),
            null_mut(),
            &mut size,
        )
    };
    if r != 0 || size == 0 {
        return None;
    }
    let mut buf = vec![0u8; size as usize];
    // SAFETY: `buf` holds `size` bytes.
    let r = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            subkey.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_BINARY,
            null_mut(),
            buf.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if r != 0 {
        return None;
    }
    buf.truncate(size as usize);
    Some(buf)
}

/// The source (adapter, id) currently driving `target`.
fn source_of(target: TargetRef) -> Result<(LUID, u32), EngineError> {
    let (paths, _) = query_raw(QDC_ONLY_ACTIVE_PATHS)?;
    paths
        .iter()
        .find(|p| luid_in(p.targetInfo.adapterId) == target.adapter && p.targetInfo.id == target.id)
        .map(|p| (p.sourceInfo.adapterId, p.sourceInfo.id))
        .ok_or(EngineError::Win32 {
            op: "find active path",
            code: dg_engine::ccd::win32::DEVICE_NOT_CONNECTED,
        })
}

/// GDI device name (`\\.\DISPLAYn`) of the source driving `target`, NUL-terminated.
fn gdi_name(target: TargetRef) -> Result<Vec<u16>, EngineError> {
    let (adapter, id) = source_of(target)?;
    // SAFETY: POD packet.
    let mut n: SOURCE_DEVICE_NAME = unsafe { std::mem::zeroed() };
    n.header = header(
        GET_SOURCE_NAME,
        size_of::<SOURCE_DEVICE_NAME>(),
        adapter,
        id,
    );
    device_info_get(&mut n.header, "DisplayConfigGetDeviceInfo(SOURCE_NAME)")?;
    Ok(wide(&from_wide(&n.viewGdiDeviceName)))
}

/// HDR state, and whether the Windows 11 24H2 HDR packets are available.
fn hdr_state(target: TargetRef) -> Result<(HdrState, bool), EngineError> {
    let adapter = luid_out(target.adapter);
    let mut v2 = ADVANCED_COLOR_INFO_2 {
        header: header(
            GET_ADVANCED_COLOR_INFO_2,
            size_of::<ADVANCED_COLOR_INFO_2>(),
            adapter,
            target.id,
        ),
        ..Default::default()
    };
    if device_info_get(&mut v2.header, "advanced color info 2").is_ok() {
        // Bits: 0 advancedColorSupported, 1 advancedColorActive, 3 limitedByPolicy,
        // 4 highDynamicRangeSupported, 5 highDynamicRangeUserEnabled.
        return Ok((
            HdrState {
                supported: v2.value & (1 << 4) != 0,
                enabled: v2.value & (1 << 5) != 0,
            },
            true,
        ));
    }
    let mut v1 = ADVANCED_COLOR_INFO {
        header: header(
            GET_ADVANCED_COLOR_INFO,
            size_of::<ADVANCED_COLOR_INFO>(),
            adapter,
            target.id,
        ),
        ..Default::default()
    };
    device_info_get(
        &mut v1.header,
        "DisplayConfigGetDeviceInfo(ADVANCED_COLOR_INFO)",
    )?;
    Ok((
        HdrState {
            supported: v1.value & 1 != 0,
            enabled: v1.value & 2 != 0,
        },
        false,
    ))
}

fn devmode() -> DEVMODEW {
    // SAFETY: POD; Windows requires dmSize to be set.
    let mut dm: DEVMODEW = unsafe { std::mem::zeroed() };
    dm.dmSize = u16::try_from(size_of::<DEVMODEW>()).expect("220 bytes");
    dm
}

fn device_mode(dm: &DEVMODEW) -> DeviceMode {
    DeviceMode {
        width: dm.dmPelsWidth,
        height: dm.dmPelsHeight,
        refresh_hz: dm.dmDisplayFrequency,
        x: dm.dmPositionX,
        y: dm.dmPositionY,
        orientation: dm.dmDisplayOrientation,
    }
}

fn len_u32(n: usize) -> u32 {
    u32::try_from(n).expect("display arrays are small")
}

// ----- conversions -----

fn luid_in(l: LUID) -> Luid {
    Luid {
        low: l.LowPart,
        high: l.HighPart,
    }
}

fn luid_out(l: Luid) -> LUID {
    LUID {
        LowPart: l.low,
        HighPart: l.high,
    }
}

fn rational_in(r: RATIONAL) -> Rational {
    Rational {
        numerator: r.Numerator,
        denominator: r.Denominator,
    }
}

fn rational_out(r: Rational) -> RATIONAL {
    RATIONAL {
        Numerator: r.numerator,
        Denominator: r.denominator,
    }
}

fn path_in(p: &PATH_INFO) -> PathInfo {
    let s = &p.sourceInfo;
    let t = &p.targetInfo;
    PathInfo {
        source: PathSource {
            adapter: luid_in(s.adapterId),
            id: s.id,
            mode_idx: s.modeInfoIdx,
            status_flags: s.statusFlags,
        },
        target: PathTarget {
            adapter: luid_in(t.adapterId),
            id: t.id,
            mode_idx: t.modeInfoIdx,
            output_technology: t.outputTechnology,
            rotation: t.rotation,
            scaling: t.scaling,
            refresh: rational_in(t.refreshRate),
            scanline_ordering: t.scanLineOrdering,
            available: t.targetAvailable != 0,
            status_flags: t.statusFlags,
        },
        flags: p.flags,
    }
}

fn path_out(p: &PathInfo) -> PATH_INFO {
    PATH_INFO {
        sourceInfo: PATH_SOURCE_INFO {
            adapterId: luid_out(p.source.adapter),
            id: p.source.id,
            modeInfoIdx: p.source.mode_idx,
            statusFlags: p.source.status_flags,
        },
        targetInfo: PATH_TARGET_INFO {
            adapterId: luid_out(p.target.adapter),
            id: p.target.id,
            modeInfoIdx: p.target.mode_idx,
            outputTechnology: p.target.output_technology,
            rotation: p.target.rotation,
            scaling: p.target.scaling,
            refreshRate: rational_out(p.target.refresh),
            scanLineOrdering: p.target.scanline_ordering,
            targetAvailable: i32::from(p.target.available),
            statusFlags: p.target.status_flags,
        },
        flags: p.flags,
    }
}

fn rd_u32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn rd_i32(b: &[u8], at: usize) -> i32 {
    i32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn rd_u64(b: &[u8], at: usize) -> u64 {
    let mut x = [0u8; 8];
    x.copy_from_slice(&b[at..at + 8]);
    u64::from_le_bytes(x)
}

fn wr(b: &mut [u8], at: usize, bytes: &[u8]) {
    b[at..at + bytes.len()].copy_from_slice(bytes);
}

/// Union layouts: source mode = width, height, pixelFormat, position.x, position.y;
/// target mode = DISPLAYCONFIG_VIDEO_SIGNAL_INFO (pixelRate, hSync, vSync, active,
/// total, videoStandard, scanLineOrdering).
fn mode_in(m: &MODE_INFO) -> ModeInfo {
    let b = &m.u.0;
    let adapter = luid_in(m.adapterId);
    match m.infoType {
        MODE_INFO_TYPE_SOURCE => ModeInfo::Source {
            adapter,
            id: m.id,
            width: rd_u32(b, 0),
            height: rd_u32(b, 4),
            pixel_format: rd_u32(b, 8),
            x: rd_i32(b, 12),
            y: rd_i32(b, 16),
        },
        MODE_INFO_TYPE_TARGET => ModeInfo::Target {
            adapter,
            id: m.id,
            signal: VideoSignal {
                pixel_rate: rd_u64(b, 0),
                h_sync: Rational {
                    numerator: rd_u32(b, 8),
                    denominator: rd_u32(b, 12),
                },
                v_sync: Rational {
                    numerator: rd_u32(b, 16),
                    denominator: rd_u32(b, 20),
                },
                active_width: rd_u32(b, 24),
                active_height: rd_u32(b, 28),
                total_width: rd_u32(b, 32),
                total_height: rd_u32(b, 36),
                video_standard: rd_u32(b, 40),
                scanline_ordering: rd_u32(b, 44),
            },
        },
        other => ModeInfo::Other {
            info_type: other,
            adapter,
            id: m.id,
            raw: b.to_vec(),
        },
    }
}

fn mode_out(m: &ModeInfo) -> MODE_INFO {
    let mut out = MODE_INFO::default();
    let b = &mut out.u.0;
    match m {
        ModeInfo::Source {
            adapter,
            id,
            width,
            height,
            pixel_format,
            x,
            y,
        } => {
            out.infoType = MODE_INFO_TYPE_SOURCE;
            out.id = *id;
            out.adapterId = luid_out(*adapter);
            wr(b, 0, &width.to_le_bytes());
            wr(b, 4, &height.to_le_bytes());
            wr(b, 8, &pixel_format.to_le_bytes());
            wr(b, 12, &x.to_le_bytes());
            wr(b, 16, &y.to_le_bytes());
        }
        ModeInfo::Target {
            adapter,
            id,
            signal: s,
        } => {
            out.infoType = MODE_INFO_TYPE_TARGET;
            out.id = *id;
            out.adapterId = luid_out(*adapter);
            wr(b, 0, &s.pixel_rate.to_le_bytes());
            wr(b, 8, &s.h_sync.numerator.to_le_bytes());
            wr(b, 12, &s.h_sync.denominator.to_le_bytes());
            wr(b, 16, &s.v_sync.numerator.to_le_bytes());
            wr(b, 20, &s.v_sync.denominator.to_le_bytes());
            wr(b, 24, &s.active_width.to_le_bytes());
            wr(b, 28, &s.active_height.to_le_bytes());
            wr(b, 32, &s.total_width.to_le_bytes());
            wr(b, 36, &s.total_height.to_le_bytes());
            wr(b, 40, &s.video_standard.to_le_bytes());
            wr(b, 44, &s.scanline_ordering.to_le_bytes());
        }
        ModeInfo::Other {
            info_type,
            adapter,
            id,
            raw,
        } => {
            out.infoType = *info_type;
            out.id = *id;
            out.adapterId = luid_out(*adapter);
            let n = raw.len().min(48);
            b[..n].copy_from_slice(&raw[..n]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_round_trip_through_the_raw_union() {
        let adapter = Luid {
            low: 85173,
            high: 0,
        };
        let source = ModeInfo::Source {
            adapter,
            id: 1,
            width: 3840,
            height: 2160,
            pixel_format: 4,
            x: -1920,
            y: 457,
        };
        assert_eq!(mode_in(&mode_out(&source)), source);
        let target = ModeInfo::Target {
            adapter,
            id: 4352,
            signal: VideoSignal {
                pixel_rate: 1_188_000_000,
                h_sync: Rational {
                    numerator: 270_000,
                    denominator: 1,
                },
                v_sync: Rational {
                    numerator: 120_000,
                    denominator: 1000,
                },
                active_width: 3840,
                active_height: 2160,
                total_width: 4400,
                total_height: 2250,
                video_standard: 255,
                scanline_ordering: 1,
            },
        };
        assert_eq!(mode_in(&mode_out(&target)), target);
    }

    #[test]
    fn query_active_paths_on_this_machine() {
        // Read-only; works on any Windows box with at least one display.
        let topo = WinBackend::new().query(Scope::Active).unwrap();
        assert!(topo.active_paths().count() >= 1);
        assert!(topo.primary_target().is_some());
    }
}
