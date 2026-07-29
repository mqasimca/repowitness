#[test]
fn every_language_allow_list_mask_selects_exact_extensions_only() {
    let cases = [
        (SourceLanguage::Rust, b"source.rs".as_slice()),
        (SourceLanguage::Go, b"source.go".as_slice()),
        (SourceLanguage::TypeScript, b"source.ts".as_slice()),
        (SourceLanguage::Tsx, b"source.tsx".as_slice()),
        (SourceLanguage::Python, b"source.py".as_slice()),
        (SourceLanguage::Python, b"source.pyi".as_slice()),
    ];

    for mask in 0_u8..32 {
        let allowed = [
            SourceLanguage::Rust,
            SourceLanguage::Go,
            SourceLanguage::TypeScript,
            SourceLanguage::Tsx,
            SourceLanguage::Python,
        ]
        .into_iter()
        .enumerate()
        .filter_map(|(index, language)| (mask & (1 << index) != 0).then_some(language))
        .collect::<BTreeSet<_>>();
        let selection =
            SelectionPolicy::SupportedLanguages(SourceLanguageSelection::from_allowed(&allowed));

        for (language, path) in cases {
            assert_eq!(
                selected_language(path, selection),
                allowed.contains(&language).then_some(language),
                "mask {mask:#07b} disagreed for {language:?}"
            );
        }
    }

    for unsupported in [
        b"source.RS".as_slice(),
        b"source.GO".as_slice(),
        b"source.TS".as_slice(),
        b"source.TSX".as_slice(),
        b"source.PY".as_slice(),
        b"source.rs.bak".as_slice(),
        b"source".as_slice(),
    ] {
        assert_eq!(
            selected_language(
                unsupported,
                SelectionPolicy::SupportedLanguages(SourceLanguageSelection::all())
            ),
            None
        );
    }
}

#[test]
fn rust_only_compatibility_selection_never_admits_other_languages() {
    assert_eq!(
        selected_language(b"source.rs", SelectionPolicy::RustOnly),
        Some(SourceLanguage::Rust)
    );
    for path in [
        b"source.go".as_slice(),
        b"source.ts".as_slice(),
        b"source.tsx".as_slice(),
        b"source.py".as_slice(),
        b"source.pyi".as_slice(),
    ] {
        assert_eq!(selected_language(path, SelectionPolicy::RustOnly), None);
    }
}
