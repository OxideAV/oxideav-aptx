//! Per-subband per-variant ADPCM tables for aptX.
//!
//! Concrete numerical tables from the spec sidecar
//! `docs/audio/aptx/data/aptx-quantizer-tables.md`. Per-subband for
//! each variant the codec needs four tables:
//!
//! - `quantize_intervals` — interval boundary thresholds
//! - `invert_quantize_dither_factors` — dither magnitudes for decode
//! - `quantize_dither_factors` — dither factors used at encode time
//! - `quantize_factor_select_offset` — per-codeword factor-select bumps
//!
//! Plus a 32-entry common `QUANTIZATION_FACTORS` step-size lookup.
//!
//! Per-subband table sizes derive from the codeword bit allocation:
//!
//! | Subband | Classic codeword | Classic table size | HD codeword | HD table size |
//! |---------|-----------------:|-------------------:|------------:|--------------:|
//! | LF      |  7               |  65                |  9          | 257           |
//! | MLF     |  4               |   9                |  6          |  33           |
//! | MHF     |  2               |   3                |  4          |   9           |
//! | HF      |  3               |   5                |  5          |  17           |
//!
//! The same numerical values appear byte-identically in two unrelated
//! open-source projects (FFmpeg `libavcodec/aptx.c` and libopenaptx by
//! Pali Rohár); the spec sidecar reproduces them as functional public
//! data. No NDA-only Qualcomm material is consulted.

/// Variant tag — selects between aptX classic (16-bit/codeword) and
/// aptX HD (24-bit/codeword).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Variant {
    Classic,
    Hd,
}

impl Variant {
    /// Block size on the wire (one stereo block).
    pub const fn block_bytes(self) -> usize {
        match self {
            Variant::Classic => 4,
            Variant::Hd => 6,
        }
    }
    /// Bits per channel per block.
    pub const fn bits_per_channel(self) -> usize {
        match self {
            Variant::Classic => 16,
            Variant::Hd => 24,
        }
    }
}

/// Subband index — must be 0/1/2/3.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Subband {
    Lf = 0,
    Mlf = 1,
    Mhf = 2,
    Hf = 3,
}

impl Subband {
    pub const fn from_index(i: usize) -> Self {
        match i {
            0 => Subband::Lf,
            1 => Subband::Mlf,
            2 => Subband::Mhf,
            _ => Subband::Hf,
        }
    }
    /// Bits used for this subband's codeword.
    pub const fn bits(self, variant: Variant) -> usize {
        match (variant, self) {
            (Variant::Classic, Subband::Lf) => 7,
            (Variant::Classic, Subband::Mlf) => 4,
            (Variant::Classic, Subband::Mhf) => 2,
            (Variant::Classic, Subband::Hf) => 3,
            (Variant::Hd, Subband::Lf) => 9,
            (Variant::Hd, Subband::Mlf) => 6,
            (Variant::Hd, Subband::Mhf) => 4,
            (Variant::Hd, Subband::Hf) => 5,
        }
    }
    /// Prediction order N per the trace doc (§4.3 table). Same for
    /// classic and HD.
    pub const fn prediction_order(self) -> usize {
        match self {
            Subband::Lf => 24,
            Subband::Mlf => 12,
            Subband::Mhf => 6,
            Subband::Hf => 12,
        }
    }
    /// Maximum `factor_select` value (per-subband cap). Same for
    /// classic and HD per the spec sidecar §"Per-subband factor_max
    /// and prediction order".
    pub const fn factor_max(self) -> i32 {
        match self {
            Subband::Lf => 0x11FF,
            Subband::Mlf => 0x14FF,
            Subband::Mhf => 0x16FF,
            Subband::Hf => 0x15FF,
        }
    }
}

/// Common 32-entry quantization-factor step-size table (`int16_t`),
/// indexed by `(factor_select & 0xFF) >> 3`. Per the spec sidecar
/// §"Common: quantization_factors[32]".
pub const QUANTIZATION_FACTORS: [i32; 32] = [
    2048, 2093, 2139, 2186, 2233, 2282, 2332, 2383, 2435, 2489, 2543, 2599, 2656, 2714, 2774, 2834,
    2896, 2960, 3025, 3091, 3158, 3228, 3298, 3371, 3444, 3520, 3597, 3676, 3756, 3838, 3922, 4008,
];

// ============================================================
// aptX classic — per-subband tables
// ============================================================

const CLASSIC_LF_INTERVALS: &[i32] = &[
    -9948, 9948, 29860, 49808, 69822, 89926, 110144, 130502, 151026, 171738, 192666, 213832,
    235264, 256982, 279014, 301384, 324118, 347244, 370790, 394782, 419250, 444226, 469742, 495832,
    522536, 549890, 577936, 606720, 636290, 666700, 698006, 730270, 763562, 797958, 833538, 870398,
    908640, 948376, 989740, 1032874, 1077948, 1125150, 1174700, 1226850, 1281900, 1340196, 1402156,
    1468282, 1539182, 1615610, 1698514, 1789098, 1888944, 2000168, 2125700, 2269750, 2438670,
    2642660, 2899462, 3243240, 3746078, 4535138, 5664098, 7102424, 8897462,
];

const CLASSIC_LF_INVERT_DITHER: &[i32] = &[
    9948, 9948, 9962, 9988, 10026, 10078, 10142, 10218, 10306, 10408, 10520, 10646, 10784, 10934,
    11098, 11274, 11462, 11664, 11880, 12112, 12358, 12618, 12898, 13194, 13510, 13844, 14202,
    14582, 14988, 15422, 15884, 16380, 16912, 17484, 18098, 18762, 19480, 20258, 21106, 22030,
    23044, 24158, 25390, 26760, 28290, 30008, 31954, 34172, 36728, 39700, 43202, 47382, 52462,
    58762, 66770, 77280, 91642, 112348, 144452, 199326, 303512, 485546, 643414, 794914, 1000124,
];

const CLASSIC_LF_DITHER: &[i32] = &[
    0, 4, 7, 10, 13, 16, 19, 22, 26, 28, 32, 35, 38, 41, 44, 47, 51, 54, 58, 62, 65, 70, 74, 79,
    84, 90, 95, 102, 109, 116, 124, 133, 143, 154, 166, 180, 195, 212, 231, 254, 279, 308, 343,
    383, 430, 487, 555, 639, 743, 876, 1045, 1270, 1575, 2002, 2628, 3591, 5177, 8026, 13719,
    26047, 45509, 39467, 37875, 51303, 0,
];

const CLASSIC_LF_FS_OFFSET: &[i32] = &[
    0, -21, -19, -17, -15, -12, -10, -8, -6, -4, -1, 1, 3, 6, 8, 10, 13, 15, 18, 20, 23, 26, 29,
    31, 34, 37, 40, 43, 47, 50, 53, 57, 60, 64, 68, 72, 76, 80, 85, 89, 94, 99, 105, 110, 116, 123,
    129, 136, 144, 152, 161, 171, 182, 194, 207, 223, 241, 263, 291, 328, 382, 467, 522, 522, 522,
];

const CLASSIC_MLF_INTERVALS: &[i32] = &[
    -89806, 89806, 278502, 494338, 759442, 1113112, 1652322, 2720256, 5190186,
];

const CLASSIC_MLF_INVERT_DITHER: &[i32] = &[
    89806, 89806, 98890, 116946, 148158, 205512, 333698, 734236, 1735696,
];

const CLASSIC_MLF_DITHER: &[i32] = &[0, 2271, 4514, 7803, 14339, 32047, 100135, 250365, 0];

const CLASSIC_MLF_FS_OFFSET: &[i32] = &[0, -14, 6, 29, 58, 96, 154, 270, 521];

const CLASSIC_MHF_INTERVALS: &[i32] = &[-194080, 194080, 890562];

const CLASSIC_MHF_INVERT_DITHER: &[i32] = &[194080, 194080, 502402];

const CLASSIC_MHF_DITHER: &[i32] = &[0, 77081, 0];

const CLASSIC_MHF_FS_OFFSET: &[i32] = &[0, -33, 136];

const CLASSIC_HF_INTERVALS: &[i32] = &[-163006, 163006, 542708, 1120554, 2669238];

const CLASSIC_HF_INVERT_DITHER: &[i32] = &[163006, 163006, 216698, 361148, 1187538];

const CLASSIC_HF_DITHER: &[i32] = &[0, 13423, 36113, 206598, 0];

const CLASSIC_HF_FS_OFFSET: &[i32] = &[0, -8, 33, 95, 262];

// ============================================================
// aptX HD — per-subband tables
// ============================================================

const HD_LF_INTERVALS: &[i32] = &[
    -2436, 2436, 7308, 12180, 17054, 21930, 26806, 31686, 36566, 41450, 46338, 51230, 56124, 61024,
    65928, 70836, 75750, 80670, 85598, 90530, 95470, 100418, 105372, 110336, 115308, 120288,
    125278, 130276, 135286, 140304, 145334, 150374, 155426, 160490, 165566, 170654, 175756, 180870,
    185998, 191138, 196294, 201466, 206650, 211850, 217068, 222300, 227548, 232814, 238096, 243396,
    248714, 254050, 259406, 264778, 270172, 275584, 281018, 286470, 291944, 297440, 302956, 308496,
    314056, 319640, 325248, 330878, 336532, 342212, 347916, 353644, 359398, 365178, 370986, 376820,
    382680, 388568, 394486, 400430, 406404, 412408, 418442, 424506, 430600, 436726, 442884, 449074,
    455298, 461554, 467844, 474168, 480528, 486922, 493354, 499820, 506324, 512866, 519446, 526064,
    532722, 539420, 546160, 552940, 559760, 566624, 573532, 580482, 587478, 594520, 601606, 608740,
    615920, 623148, 630426, 637754, 645132, 652560, 660042, 667576, 675164, 682808, 690506, 698262,
    706074, 713946, 721876, 729868, 737920, 746036, 754216, 762460, 770770, 779148, 787594, 796108,
    804694, 813354, 822086, 830892, 839774, 848736, 857776, 866896, 876100, 885386, 894758, 904218,
    913766, 923406, 933138, 942964, 952886, 962908, 973030, 983254, 993582, 1004020, 1014566,
    1025224, 1035996, 1046886, 1057894, 1069026, 1080284, 1091670, 1103186, 1114838, 1126628,
    1138558, 1150634, 1162858, 1175236, 1187768, 1200462, 1213320, 1226346, 1239548, 1252928,
    1266490, 1280242, 1294188, 1308334, 1322688, 1337252, 1352034, 1367044, 1382284, 1397766,
    1413494, 1429478, 1445728, 1462252, 1479058, 1496158, 1513562, 1531280, 1549326, 1567710,
    1586446, 1605550, 1625034, 1644914, 1665208, 1685932, 1707108, 1728754, 1750890, 1773542,
    1796732, 1820488, 1844840, 1869816, 1895452, 1921780, 1948842, 1976680, 2005338, 2034868,
    2065322, 2096766, 2129260, 2162880, 2197708, 2233832, 2271352, 2310384, 2351050, 2393498,
    2437886, 2484404, 2533262, 2584710, 2639036, 2696578, 2757738, 2822998, 2892940, 2968278,
    3049896, 3138912, 3236760, 3345312, 3467068, 3605434, 3765154, 3952904, 4177962, 4452178,
    4787134, 5187290, 5647128, 6159120, 6720518, 7332904, 8000032, 8726664, 9518152, 10380372,
];

const HD_LF_INVERT_DITHER: &[i32] = &[
    2436, 2436, 2436, 2436, 2438, 2438, 2438, 2440, 2442, 2442, 2444, 2446, 2448, 2450, 2454, 2456,
    2458, 2462, 2464, 2468, 2472, 2476, 2480, 2484, 2488, 2492, 2498, 2502, 2506, 2512, 2518, 2524,
    2528, 2534, 2540, 2548, 2554, 2560, 2568, 2574, 2582, 2588, 2596, 2604, 2612, 2620, 2628, 2636,
    2646, 2654, 2664, 2672, 2682, 2692, 2702, 2712, 2722, 2732, 2742, 2752, 2764, 2774, 2786, 2798,
    2810, 2822, 2834, 2846, 2858, 2870, 2884, 2896, 2910, 2924, 2938, 2952, 2966, 2980, 2994, 3010,
    3024, 3040, 3056, 3070, 3086, 3104, 3120, 3136, 3154, 3170, 3188, 3206, 3224, 3242, 3262, 3280,
    3300, 3320, 3338, 3360, 3380, 3400, 3422, 3442, 3464, 3486, 3508, 3532, 3554, 3578, 3602, 3626,
    3652, 3676, 3702, 3728, 3754, 3780, 3808, 3836, 3864, 3892, 3920, 3950, 3980, 4010, 4042, 4074,
    4106, 4138, 4172, 4206, 4240, 4276, 4312, 4348, 4384, 4422, 4460, 4500, 4540, 4580, 4622, 4664,
    4708, 4752, 4796, 4842, 4890, 4938, 4986, 5036, 5086, 5138, 5192, 5246, 5300, 5358, 5416, 5474,
    5534, 5596, 5660, 5726, 5792, 5860, 5930, 6002, 6074, 6150, 6226, 6306, 6388, 6470, 6556, 6644,
    6736, 6828, 6924, 7022, 7124, 7228, 7336, 7448, 7562, 7680, 7802, 7928, 8058, 8192, 8332, 8476,
    8624, 8780, 8940, 9106, 9278, 9458, 9644, 9840, 10042, 10252, 10472, 10702, 10942, 11194,
    11458, 11734, 12024, 12328, 12648, 12986, 13342, 13720, 14118, 14540, 14990, 15466, 15976,
    16520, 17102, 17726, 18398, 19124, 19908, 20760, 21688, 22702, 23816, 25044, 26404, 27922,
    29622, 31540, 33720, 36222, 39116, 42502, 46514, 51334, 57218, 64536, 73830, 85890, 101860,
    123198, 151020, 183936, 216220, 243618, 268374, 293022, 319362, 347768, 378864, 412626, 449596,
];

// HD LF dither table is 256 entries (one fewer than the interval table).
const HD_LF_DITHER: &[i32] = &[
    0, 0, 0, 1, 0, 0, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 1, 1, 2, 2, 2, 1,
    2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 3, 2, 3, 2, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3,
    3, 3, 3, 3, 3, 4, 3, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 5, 4, 4, 5, 4, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 6, 5, 5, 6, 5, 6, 6, 6, 6, 6, 6, 6, 6, 7, 6, 7, 7, 7, 7, 7, 7, 7, 7, 7, 8, 8, 8, 8, 8, 8,
    8, 9, 9, 9, 9, 9, 9, 9, 10, 10, 10, 10, 10, 11, 11, 11, 11, 11, 12, 12, 12, 12, 13, 13, 13, 14,
    14, 14, 15, 15, 15, 15, 16, 16, 17, 17, 17, 18, 18, 18, 19, 19, 20, 21, 21, 22, 22, 23, 23, 24,
    25, 26, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 39, 40, 42, 43, 45, 47, 49, 51, 53, 55,
    58, 60, 63, 66, 69, 73, 76, 80, 85, 89, 95, 100, 106, 113, 119, 128, 136, 146, 156, 168, 182,
    196, 213, 232, 254, 279, 307, 340, 380, 425, 480, 545, 626, 724, 847, 1003, 1205, 1471, 1830,
    2324, 3015, 3993, 5335, 6956, 8229, 8071, 6850, 6189, 6162, 6585, 7102, 7774, 8441, 9243,
];

const HD_LF_FS_OFFSET: &[i32] = &[
    0, -22, -21, -21, -20, -20, -19, -19, -18, -18, -17, -17, -16, -16, -15, -14, -14, -13, -13,
    -12, -12, -11, -11, -10, -10, -9, -9, -8, -7, -7, -6, -6, -5, -5, -4, -4, -3, -3, -2, -1, -1,
    0, 0, 1, 1, 2, 2, 3, 4, 4, 5, 5, 6, 6, 7, 8, 8, 9, 9, 10, 11, 11, 12, 12, 13, 14, 14, 15, 15,
    16, 17, 17, 18, 19, 19, 20, 20, 21, 22, 22, 23, 24, 24, 25, 26, 26, 27, 28, 28, 29, 30, 30, 31,
    32, 33, 33, 34, 35, 35, 36, 37, 38, 38, 39, 40, 41, 41, 42, 43, 44, 44, 45, 46, 47, 48, 48, 49,
    50, 51, 52, 52, 53, 54, 55, 56, 57, 58, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 69, 70,
    71, 72, 73, 74, 75, 77, 78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 89, 90, 91, 92, 93, 94, 96, 97,
    98, 99, 101, 102, 103, 105, 106, 107, 109, 110, 112, 113, 115, 116, 118, 119, 121, 122, 124,
    125, 127, 129, 130, 132, 134, 136, 137, 139, 141, 143, 145, 147, 149, 151, 153, 155, 158, 160,
    162, 164, 167, 169, 172, 174, 177, 180, 182, 185, 188, 191, 194, 197, 201, 204, 208, 211, 215,
    219, 223, 227, 232, 236, 241, 246, 251, 257, 263, 269, 275, 283, 290, 298, 307, 317, 327, 339,
    352, 367, 384, 404, 429, 458, 494, 522, 522, 522, 522, 522, 522, 522, 522, 522,
];

const HD_MLF_INTERVALS: &[i32] = &[
    -21236, 21236, 63830, 106798, 150386, 194832, 240376, 287258, 335726, 386034, 438460, 493308,
    550924, 611696, 676082, 744626, 817986, 896968, 982580, 1076118, 1179278, 1294344, 1424504,
    1574386, 1751090, 1966260, 2240868, 2617662, 3196432, 4176450, 5658260, 7671068, 10380372,
];

const HD_MLF_INVERT_DITHER: &[i32] = &[
    21236, 21236, 21360, 21608, 21978, 22468, 23076, 23806, 24660, 25648, 26778, 28070, 29544,
    31228, 33158, 35386, 37974, 41008, 44606, 48934, 54226, 60840, 69320, 80564, 96140, 119032,
    155576, 221218, 357552, 622468, 859344, 1153464, 1555840,
];

// HD MLF dither table is 32 entries.
const HD_MLF_DITHER: &[i32] = &[
    0, 31, 62, 93, 123, 152, 183, 214, 247, 283, 323, 369, 421, 483, 557, 647, 759, 900, 1082,
    1323, 1654, 2120, 2811, 3894, 5723, 9136, 16411, 34084, 66229, 59219, 73530, 100594,
];

const HD_MLF_FS_OFFSET: &[i32] = &[
    0, -21, -16, -12, -7, -2, 3, 8, 13, 19, 24, 30, 36, 43, 50, 57, 65, 74, 83, 93, 104, 117, 131,
    147, 166, 189, 219, 259, 322, 427, 521, 521, 521,
];

const HD_MHF_INTERVALS: &[i32] = &[
    -95044, 95044, 295844, 528780, 821332, 1226438, 1890540, 3344850, 6450664,
];

const HD_MHF_INVERT_DITHER: &[i32] = &[
    95044, 95044, 105754, 127180, 165372, 39736, 424366, 1029946, 2075866,
];

// HD MHF dither table is 8 entries.
const HD_MHF_DITHER: &[i32] = &[0, 2678, 5357, 9548, -31409, 96158, 151395, 261480];

const HD_MHF_FS_OFFSET: &[i32] = &[0, -17, 5, 30, 62, 105, 177, 334, 518];

const HD_HF_INTERVALS: &[i32] = &[
    -45754, 45754, 138496, 234896, 337336, 448310, 570738, 708380, 866534, 1053262, 1281958,
    1577438, 1993050, 2665984, 3900982, 5902844, 8897462,
];

const HD_HF_INVERT_DITHER: &[i32] = &[
    45754, 45754, 46988, 49412, 53026, 57950, 64478, 73164, 84988, 101740, 126958, 168522, 247092,
    425842, 809154, 1192708, 1801910,
];

// HD HF dither table is 16 entries.
const HD_HF_DITHER: &[i32] = &[
    0, 309, 606, 904, 1231, 1632, 2172, 2956, 4188, 6305, 10391, 19643, 44688, 95828, 95889, 152301,
];

const HD_HF_FS_OFFSET: &[i32] = &[
    0, -18, -8, 2, 13, 25, 38, 53, 70, 90, 115, 147, 192, 264, 398, 521, 521,
];

/// Bundle of per-subband tables for a given (variant, subband) pair.
///
/// `intervals`, `invert_quantize_dither_factors`, and
/// `factor_select_offsets` all have the same length (= `2^(bits-1) +
/// 1`); `quantize_dither_factors` may be one shorter for HD because
/// its sentinel column is padded differently — the encoder uses
/// `quantize_dither_factors` indexed by codeword magnitude, while
/// the decoder uses `invert_quantize_dither_factors` indexed by the
/// boundary index.
#[derive(Clone, Debug)]
pub struct SubbandTables {
    /// Quantizer interval boundaries (indexed by codeword magnitude).
    pub intervals: &'static [i32],
    /// Dither factors used by the decoder when inverting the quantizer.
    pub invert_quantize_dither_factors: &'static [i32],
    /// Dither factors used at encode time, indexed by codeword magnitude.
    /// May be one shorter than `intervals` (HD pads its sentinel column).
    pub quantize_dither_factors: &'static [i32],
    /// Per-codeword offsets driving the leaky-integrator
    /// `factor_select` update.
    pub factor_select_offsets: &'static [i32],
}

impl SubbandTables {
    pub fn new(variant: Variant, sb: Subband) -> Self {
        let (intervals, invd, dith, fso): (
            &'static [i32],
            &'static [i32],
            &'static [i32],
            &'static [i32],
        ) = match (variant, sb) {
            (Variant::Classic, Subband::Lf) => (
                CLASSIC_LF_INTERVALS,
                CLASSIC_LF_INVERT_DITHER,
                CLASSIC_LF_DITHER,
                CLASSIC_LF_FS_OFFSET,
            ),
            (Variant::Classic, Subband::Mlf) => (
                CLASSIC_MLF_INTERVALS,
                CLASSIC_MLF_INVERT_DITHER,
                CLASSIC_MLF_DITHER,
                CLASSIC_MLF_FS_OFFSET,
            ),
            (Variant::Classic, Subband::Mhf) => (
                CLASSIC_MHF_INTERVALS,
                CLASSIC_MHF_INVERT_DITHER,
                CLASSIC_MHF_DITHER,
                CLASSIC_MHF_FS_OFFSET,
            ),
            (Variant::Classic, Subband::Hf) => (
                CLASSIC_HF_INTERVALS,
                CLASSIC_HF_INVERT_DITHER,
                CLASSIC_HF_DITHER,
                CLASSIC_HF_FS_OFFSET,
            ),
            (Variant::Hd, Subband::Lf) => (
                HD_LF_INTERVALS,
                HD_LF_INVERT_DITHER,
                HD_LF_DITHER,
                HD_LF_FS_OFFSET,
            ),
            (Variant::Hd, Subband::Mlf) => (
                HD_MLF_INTERVALS,
                HD_MLF_INVERT_DITHER,
                HD_MLF_DITHER,
                HD_MLF_FS_OFFSET,
            ),
            (Variant::Hd, Subband::Mhf) => (
                HD_MHF_INTERVALS,
                HD_MHF_INVERT_DITHER,
                HD_MHF_DITHER,
                HD_MHF_FS_OFFSET,
            ),
            (Variant::Hd, Subband::Hf) => (
                HD_HF_INTERVALS,
                HD_HF_INVERT_DITHER,
                HD_HF_DITHER,
                HD_HF_FS_OFFSET,
            ),
        };
        Self {
            intervals,
            invert_quantize_dither_factors: invd,
            quantize_dither_factors: dith,
            factor_select_offsets: fso,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classic_table_sizes_match_trace_doc() {
        // Per trace doc §9.1: LF=65, MLF=9, MHF=3, HF=5.
        let expected: [(Subband, usize); 4] = [
            (Subband::Lf, 65),
            (Subband::Mlf, 9),
            (Subband::Mhf, 3),
            (Subband::Hf, 5),
        ];
        for (sb, n) in expected {
            let t = SubbandTables::new(Variant::Classic, sb);
            assert_eq!(t.intervals.len(), n, "{sb:?} intervals");
            assert_eq!(
                t.invert_quantize_dither_factors.len(),
                n,
                "{sb:?} invert dither"
            );
            assert_eq!(
                t.quantize_dither_factors.len(),
                n,
                "{sb:?} quant dither (classic = full size)"
            );
            assert_eq!(t.factor_select_offsets.len(), n, "{sb:?} fs_offsets");
        }
    }

    #[test]
    fn hd_table_sizes_match_trace_doc() {
        // Per trace doc §9.1: LF=257, MLF=33, MHF=9, HF=17.
        // For HD, quantize_dither_factors is one shorter than intervals.
        let expected: [(Subband, usize, usize); 4] = [
            // (subband, intervals/invert/fs len, quantize_dither_factors len)
            (Subband::Lf, 257, 256),
            (Subband::Mlf, 33, 32),
            (Subband::Mhf, 9, 8),
            (Subband::Hf, 17, 16),
        ];
        for (sb, n, n_dith) in expected {
            let t = SubbandTables::new(Variant::Hd, sb);
            assert_eq!(t.intervals.len(), n, "{sb:?} intervals");
            assert_eq!(t.invert_quantize_dither_factors.len(), n, "{sb:?} invert");
            assert_eq!(t.quantize_dither_factors.len(), n_dith, "{sb:?} quant dith");
            assert_eq!(t.factor_select_offsets.len(), n, "{sb:?} fs_offsets");
        }
    }

    #[test]
    fn intervals_are_monotonic() {
        // Spec sidecar: intervals are positive and strictly increasing
        // from index 1 onwards (index 0 is a negative sentinel).
        for v in [Variant::Classic, Variant::Hd] {
            for sb in [Subband::Lf, Subband::Mlf, Subband::Mhf, Subband::Hf] {
                let t = SubbandTables::new(v, sb);
                for w in t.intervals[1..].windows(2) {
                    assert!(
                        w[0] < w[1],
                        "intervals not strictly monotonic in {v:?}/{sb:?}: {w:?}"
                    );
                }
                // index 0 should be negative.
                assert!(
                    t.intervals[0] < 0,
                    "intervals[0] not negative sentinel in {v:?}/{sb:?}: {}",
                    t.intervals[0]
                );
            }
        }
    }

    #[test]
    fn variant_block_sizes() {
        assert_eq!(Variant::Classic.block_bytes(), 4);
        assert_eq!(Variant::Hd.block_bytes(), 6);
        assert_eq!(Variant::Classic.bits_per_channel(), 16);
        assert_eq!(Variant::Hd.bits_per_channel(), 24);
    }

    #[test]
    fn quantization_factors_are_monotonic_increasing() {
        for w in QUANTIZATION_FACTORS.windows(2) {
            assert!(w[0] <= w[1]);
        }
    }

    #[test]
    fn factor_max_matches_spec() {
        // Per spec sidecar §"Per-subband factor_max and prediction order".
        assert_eq!(Subband::Lf.factor_max(), 0x11FF);
        assert_eq!(Subband::Mlf.factor_max(), 0x14FF);
        assert_eq!(Subband::Mhf.factor_max(), 0x16FF);
        assert_eq!(Subband::Hf.factor_max(), 0x15FF);
    }

    #[test]
    fn quantization_factor_table_first_and_last_match_spec() {
        assert_eq!(QUANTIZATION_FACTORS[0], 2048);
        assert_eq!(QUANTIZATION_FACTORS[31], 4008);
    }

    #[test]
    fn classic_lf_first_interval_matches_spec() {
        let t = SubbandTables::new(Variant::Classic, Subband::Lf);
        assert_eq!(t.intervals[0], -9948);
        assert_eq!(t.intervals[1], 9948);
        assert_eq!(t.intervals[64], 8_897_462);
    }

    #[test]
    fn hd_lf_last_interval_matches_spec() {
        let t = SubbandTables::new(Variant::Hd, Subband::Lf);
        assert_eq!(t.intervals[0], -2436);
        assert_eq!(t.intervals[256], 10_380_372);
    }
}
