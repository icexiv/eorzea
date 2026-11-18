# iced_tiny_skia local patch

Source: crates.io `iced_tiny_skia` 0.14.1, from the MIT-licensed
[Iced project](https://github.com/iced-rs/iced). `src/` and the normalized
`Cargo.toml` are copied from that release; `LICENSE` is Iced's MIT license.

Local change in `src/engine.rs`: always apply the text editor's clip mask.
Its bounds represent the editor viewport, not the extent of partially visible
glyphs. Skipping the mask when those bounds fit the clip lets glyphs spill into
the log drawer's padding, where incremental redraws can leave overlapping text.

Regression coverage: `ui::log_view::tests::scrolled_logs_do_not_draw_outside_the_editor`
in `eorzea-app`, using CPU-only rendering without a window.
