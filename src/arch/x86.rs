use raw_cpuid::CpuId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum X86Level {
    X86_64,
    V2,
    V3,
    V4,
}

pub fn detect_x86_level() -> X86Level {
    let cpuid = CpuId::new();

    let Some(fi) = cpuid.get_feature_info() else {
        return X86Level::X86_64;
    };

    let Some(epfi) = cpuid.get_extended_processor_and_feature_identifiers() else {
        return X86Level::X86_64;
    };

    let has_v2 = fi.has_sse3()
        && fi.has_ssse3()
        && fi.has_sse41()
        && fi.has_sse42()
        && fi.has_popcnt()
        && fi.has_cmpxchg16b()
        && epfi.has_lahf_sahf();

    if !has_v2 {
        return X86Level::X86_64;
    }

    let Some(efi) = cpuid.get_extended_feature_info() else {
        return X86Level::V2;
    };

    let has_v3 = fi.has_avx()
        && fi.has_fma()
        && fi.has_f16c()
        && fi.has_movbe()
        && fi.has_xsave()
        && efi.has_avx2()
        && efi.has_bmi1()
        && efi.has_bmi2()
        && epfi.has_lzcnt();

    if !has_v3 {
        return X86Level::V2;
    }

    let has_v4 = efi.has_avx512f()
        && efi.has_avx512bw()
        && efi.has_avx512cd()
        && efi.has_avx512dq()
        && efi.has_avx512vl();

    if has_v4 { X86Level::V4 } else { X86Level::V3 }
}
