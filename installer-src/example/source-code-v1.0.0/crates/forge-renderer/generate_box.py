def gen():
    # Format: (up, right, down, left, rounded, double_type)
    # 0=none, 1=light, 2=heavy, 3=double, 4=dashed (maybe implement later, treat as 1 for now)
    table = {}
    
    # Defaults
    for i in range(128):
        table[i] = (0,0,0,0,0)

    # basic lines
    table[0x00] = (0,1,0,1,0) # ─
    table[0x01] = (0,2,0,2,0) # ━
    table[0x02] = (1,0,1,0,0) # │
    table[0x03] = (2,0,2,0,0) # ┃
    table[0x04] = (0,1,0,1,0) # ┄ (approx)
    table[0x05] = (0,2,0,2,0) # ┅
    table[0x06] = (1,0,1,0,0) # ┆
    table[0x07] = (2,0,2,0,0) # ┇
    table[0x08] = (0,1,0,1,0) # ┈
    table[0x09] = (0,2,0,2,0) # ┉
    table[0x0A] = (1,0,1,0,0) # ┊
    table[0x0B] = (2,0,2,0,0) # ┋
    
    # corners
    table[0x0C] = (0,1,1,0,0) # ┌
    table[0x0D] = (0,2,1,0,0) # ┍
    table[0x0E] = (0,1,2,0,0) # ┎
    table[0x0F] = (0,2,2,0,0) # ┏

    table[0x10] = (0,0,1,1,0) # ┐
    table[0x11] = (0,0,1,2,0) # ┑
    table[0x12] = (0,0,2,1,0) # ┒
    table[0x13] = (0,0,2,2,0) # ┓

    table[0x14] = (1,1,0,0,0) # └
    table[0x15] = (1,2,0,0,0) # ┕
    table[0x16] = (2,1,0,0,0) # ┖
    table[0x17] = (2,2,0,0,0) # ┗

    table[0x18] = (1,0,0,1,0) # ┘
    table[0x19] = (1,0,0,2,0) # ┙
    table[0x1A] = (2,0,0,1,0) # ┚
    table[0x1B] = (2,0,0,2,0) # ┛

    # tees
    table[0x1C] = (1,1,1,0,0) # ├
    table[0x1D] = (1,2,1,0,0) # ┝
    table[0x1E] = (2,1,1,0,0) # ┞
    table[0x1F] = (1,1,2,0,0) # ┟
    table[0x20] = (2,1,2,0,0) # ┠
    table[0x21] = (2,2,1,0,0) # ┡
    table[0x22] = (1,2,2,0,0) # ┢
    table[0x23] = (2,2,2,0,0) # ┣

    table[0x24] = (1,0,1,1,0) # ┤
    table[0x25] = (1,0,1,2,0) # ┥
    table[0x26] = (2,0,1,1,0) # ┦
    table[0x27] = (1,0,2,1,0) # ┧
    table[0x28] = (2,0,2,1,0) # ┨
    table[0x29] = (2,0,1,2,0) # ┩
    table[0x2A] = (1,0,2,2,0) # ┪
    table[0x2B] = (2,0,2,2,0) # ┫

    table[0x2C] = (0,1,1,1,0) # ┬
    table[0x2D] = (0,2,1,1,0) # ┭
    table[0x2E] = (0,1,1,2,0) # ┮
    table[0x2F] = (0,2,1,2,0) # ┯
    table[0x30] = (0,1,2,1,0) # ┰
    table[0x31] = (0,2,2,1,0) # ┱
    table[0x32] = (0,1,2,2,0) # ┲
    table[0x33] = (0,2,2,2,0) # ┳

    table[0x34] = (1,1,0,1,0) # ┴
    table[0x35] = (1,2,0,1,0) # ┵
    table[0x36] = (1,1,0,2,0) # ┶
    table[0x37] = (1,2,0,2,0) # ┷
    table[0x38] = (2,1,0,1,0) # ┸
    table[0x39] = (2,2,0,1,0) # ┹
    table[0x3A] = (2,1,0,2,0) # ┺
    table[0x3B] = (2,2,0,2,0) # ┻

    # cross
    table[0x3C] = (1,1,1,1,0) # ┼
    table[0x3D] = (1,2,1,1,0) # ┽
    table[0x3E] = (1,1,1,2,0) # ┾
    table[0x3F] = (1,2,1,2,0) # ┿
    table[0x40] = (2,1,1,1,0) # ╀
    table[0x41] = (1,1,2,1,0) # ╁
    table[0x42] = (2,1,2,1,0) # ╂
    table[0x43] = (1,2,1,2,0) # ╃
    table[0x44] = (2,2,1,1,0) # ╄
    table[0x45] = (1,2,2,1,0) # ╅
    table[0x46] = (2,1,1,2,0) # ╆
    table[0x47] = (1,1,2,2,0) # ╇
    table[0x48] = (2,2,2,1,0) # ╈
    table[0x49] = (2,1,2,2,0) # ╉
    table[0x4A] = (1,2,2,2,0) # ╊
    table[0x4B] = (2,2,2,2,0) # ╋

    # double lines
    table[0x50] = (0,3,0,3,0) # ═
    table[0x51] = (3,0,3,0,0) # ║
    table[0x52] = (0,3,1,0,0) # ╒
    table[0x53] = (0,1,3,0,0) # ╓
    table[0x54] = (0,3,3,0,0) # ╔
    table[0x55] = (0,0,1,3,0) # ╕
    table[0x56] = (0,0,3,1,0) # ╖
    table[0x57] = (0,0,3,3,0) # ╗
    table[0x58] = (1,3,0,0,0) # ╘
    table[0x59] = (3,1,0,0,0) # ╙
    table[0x5A] = (3,3,0,0,0) # ╚
    table[0x5B] = (1,0,0,3,0) # ╛
    table[0x5C] = (3,0,0,1,0) # ╜
    table[0x5D] = (3,0,0,3,0) # ╝
    table[0x5E] = (1,3,1,0,0) # ╞
    table[0x5F] = (3,1,3,0,0) # ╟
    table[0x60] = (3,3,3,0,0) # ╠
    table[0x61] = (1,0,1,3,0) # ╡
    table[0x62] = (3,0,3,1,0) # ╢
    table[0x63] = (3,0,3,3,0) # ╣
    table[0x64] = (0,3,1,3,0) # ╤
    table[0x65] = (0,1,3,1,0) # ╥
    table[0x66] = (0,3,3,3,0) # ╦
    table[0x67] = (1,3,0,3,0) # ╧
    table[0x68] = (3,1,0,1,0) # ╨
    table[0x69] = (3,3,0,3,0) # ╩
    table[0x6A] = (1,3,1,3,0) # ╪
    table[0x6B] = (3,1,3,1,0) # ╫
    table[0x6C] = (3,3,3,3,0) # ╬

    # rounded
    table[0x6D] = (0,1,1,0,1) # ╭
    table[0x6E] = (0,0,1,1,1) # ╮
    table[0x6F] = (1,0,0,1,1) # ╯
    table[0x70] = (1,1,0,0,1) # ╰

    # half lines
    table[0x74] = (0,0,0,1,0) # ╴
    table[0x75] = (1,0,0,0,0) # ╵
    table[0x76] = (0,1,0,0,0) # ╶
    table[0x77] = (0,0,1,0,0) # ╷
    table[0x78] = (0,0,0,2,0) # ╸
    table[0x79] = (2,0,0,0,0) # ╹
    table[0x7A] = (0,2,0,0,0) # ╺
    table[0x7B] = (0,0,2,0,0) # ╻
    table[0x7C] = (0,1,0,2,0) # ╼
    table[0x7D] = (1,0,2,0,0) # ╽
    table[0x7E] = (0,2,0,1,0) # ╾
    table[0x7F] = (2,0,1,0,0) # ╿

    print("pub fn decode_box_draw(c: u32) -> u32 {")
    print("    match c {")
    for i in range(128):
        u, r, d, l, rnd = table[i]
        if u==0 and r==0 and d==0 and l==0 and rnd==0:
            continue
        # bits: U (0-2), R (3-5), D (6-8), L (9-11), Rnd (12)
        bits = (u) | (r << 3) | (d << 6) | (l << 9) | (rnd << 12)
        print(f"        0x{0x2500 + i:04X} => {bits},")
    print("        _ => 0,")
    print("    }")
    print("}")

gen()
