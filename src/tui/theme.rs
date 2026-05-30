use ratatui::style::Color;

pub struct Theme {
    pub bg: Color,
    pub blue: Color,
    pub fg: Color,
    pub grey2: Color,
    pub purple: Color,
    pub aqua: Color,
    pub green: Color,
    pub yellow: Color,
    pub red: Color,
    pub orange: Color,
    #[allow(dead_code)]
    pub grey0: Color,
}

impl Theme {
    pub fn everforest() -> Self {
        Self {
            bg: Color::from_u32(0x001e2326),
            blue: Color::from_u32(0x007fbbb3),
            fg: Color::from_u32(0x00d3c6aa),
            grey2: Color::from_u32(0x00939f91),
            purple: Color::from_u32(0x00d699b6),
            aqua: Color::from_u32(0x0083c092),
            green: Color::from_u32(0x00a7c080),
            yellow: Color::from_u32(0x00dbbc7f),
            red: Color::from_u32(0x00e67e80),
            orange: Color::from_u32(0x00e69875),
            grey0: Color::from_u32(0x007a8478),
        }
    }
}
