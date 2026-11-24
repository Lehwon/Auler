use slint_build::DefaultTranslationContext;

fn main() {
    let config = slint_build::CompilerConfiguration::new()
        // 시스템 다크 모드와 관계없이 명세서의 밝은 테마로 시작한다.
        .with_style("fluent-light".into())
        // 번역 파일: i18n/<언어>/LC_MESSAGES/auler.po. 원문(msgid)은 영어다.
        .with_bundled_translations("../i18n")
        // 컴포넌트 이름을 번역 문맥으로 쓰지 않는다. 같은 문구는 한 번만 번역한다.
        .with_default_translation_context(DefaultTranslationContext::None);
    slint_build::compile_with_config("ui/main.slint", config).expect("Slint UI 컴파일 실패");
}
