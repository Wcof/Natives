//! 趋势分析五大核心量化评估模块。
//!
//! 包含：
//! - `trend`: 均线趋势（MA20/MA60 多空排列与上升趋势线拟合）
//! - `breakout`: 海龟突破（20日/55日唐奇安通道与 2N 止损）
//! - `volume_price`: 量价模式（价量九宫格与 OBV 资金流修正）
//! - `pattern`: 几何形态（头肩底/顶、双底/顶、三角形、箱体、旗形、跳空、双弧底）
//! - `canslim`: 欧奈尔七维度动量评分与杯柄形态检测

pub mod breakout;
pub mod canslim;
pub mod pattern;
pub mod trend;
pub mod volume_price;
