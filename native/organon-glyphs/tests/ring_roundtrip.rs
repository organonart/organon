//! End to end in one process: a real `ttfx` effect → the producer's walk → the glyph
//! ring → the reader the world uses. The binary's loop is thin around exactly this, so
//! this is the test that says the three crates agree about the bytes.

use organon_core::glyph_ring::{
    frame_name, set_frame_name, GlyphFrame, GlyphRingReader, GlyphRingWriter, FRAME_SETTLED,
    SGR_HAS_FG, TTFX_CELL_ASPECT,
};
use organon_glyphs::Producer;

#[test]
fn a_producer_publishes_and_the_world_reads_the_same_grid_back() {
    let ns = format!("glyph-rt-{}", std::process::id());
    let path = organon_core::ipc::glyph_ring_path_in(&ns).unwrap();
    let _ = std::fs::remove_file(&path);

    // Asymmetric on purpose: a wrong flip must show. ⚠️ No trailing spaces — ttfx
    // trims them, so "█▀ " is a 2-wide line, not 3, and the canvas follows the widest
    // line as trimmed. (Found by this test asserting 3 and getting 2.)
    let input = "█▀\n▄";
    let mut p = Producer::start(input, "overflow", 3, 60, None, None).unwrap();
    let mut w = GlyphRingWriter::create_ns(&ns, TTFX_CELL_ASPECT, 60.0).unwrap();
    let r = GlyphRingReader::open_ns(&ns).unwrap();
    assert!(r.is_open(), "the writer created the file before the reader opened it");
    assert!(r.latest().is_none(), "nothing published yet");

    let mut cells = Vec::new();
    let mut meta = GlyphFrame::default();
    set_frame_name(&mut meta, "overflow");
    let mut ticks = 0;
    while p.step() {
        let (cols, rows) = p.walk(&mut cells);
        meta.cols = cols;
        meta.rows = rows;
        meta.tick = p.tick;
        w.publish(&meta, &cells).unwrap();
        ticks += 1;
        // Every published frame is readable, whole, and identical to the walk.
        let g = r.latest().expect("published");
        assert_eq!(g.frame.seq, ticks);
        assert_eq!((g.cols(), g.rows()), (2, 2));
        assert_eq!(g.cells, cells);
        assert_eq!(g.cell_aspect, TTFX_CELL_ASPECT);
    }
    // Settle: publish the final walk flagged, then a heartbeat with the same bytes.
    let (cols, rows) = p.walk(&mut cells);
    meta.cols = cols;
    meta.rows = rows;
    meta.flags = FRAME_SETTLED;
    w.publish(&meta, &cells).unwrap();
    let settled = r.latest().unwrap();
    w.publish(&meta, &cells).unwrap();
    let beat = r.latest().unwrap();
    assert!(settled.settled() && beat.settled());
    assert_eq!(beat.frame.seq, settled.frame.seq + 1, "a heartbeat is a publish");
    assert_eq!(beat.frame.generation, settled.frame.generation, "…but not a change");
    assert_eq!(frame_name(&beat.frame), "overflow");
    // The settled grid is the input, top-down, and it is coloured.
    let syms: String = beat.cells.iter().map(|c| char::from_u32(c.symbol).unwrap()).collect();
    assert_eq!(syms, "█▀▄ ");
    assert!(beat.cells.iter().filter(|c| c.symbol != ' ' as u32).all(|c| c.sgr & SGR_HAS_FG != 0));

    let _ = std::fs::remove_file(&path);
}
