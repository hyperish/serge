# 01. DX 및 DSL 설계 (serge)

> 상태: 뷰 마크업 `ui!` 5대 최소 문법 · 기능 조립 `feature()` 체이닝 · `#[server]` 인라인 서버 함수 호출 · CRA급 데브 서버 내장은 **확정**, 추가 세부 속성은 **제안** · 기준일: 2026-11-25

`serge`의 DX 목표는 **웹(DOM)이든 네이티브(GPU/CPU, 데스크톱·iOS·Android)든 완전히 동일한 proc macro UI DSL(`ui!`)로 화면을 구축**하고, 기능·상태·라우트 조립은 `orbic::component()`와 대칭되는 `serge::feature()` Rust 체이닝으로 묶는 것입니다.

## 1. 표현 역할 고정 및 `ui!` 5대 최소 문법 규격 (확정, 2026-11-19)

- **뷰(UI 트리) 마크업은 `ui!` Proc Macro DSL로 고정**:
  1. **위젯 선언**: `Widget { prop: expr, ... Child1 { ... } Child2 { ... } }` (프로퍼티는 `,`로 구분, 자식은 블록 나열).
  2. **텍스트 리프 & 시그널 보간**: `Text { "Count: {count()}" }` (단일 포지셔널 포맷 문자열, 시그널 읽기는 `{sig()}` 함수 호출로 통일).
  3. **이벤트·콜백 화살표 통일**: 모든 이벤트/콜백은 `event(args) => { ... }` (비동기 핸들러는 `clicked => async move { ... }`)로 통일하며 `on_click: move |...|` 혼용을 금지합니다.
  4. **표준 Rust 제어문**: 조건·반복은 표준 Rust 문법 그대로 `if cond { ... } else { ... }`, `for item in list() { key: item.id, ... }`를 사용합니다.
  5. **타깃 전용 속성 네임스페이스**: 공통 위젯(`View`, `Stack`, `Card`, `Text`, `Button`, `Input`, `Canvas` 등) 위에 웹 전용은 `web::*`, 네이티브 전용은 `native::*`로 컴파일 타임 검증합니다.
- **도메인·라우트·가드 조립은 `serge::feature()` 순수 Rust 체이닝으로 고정**:
  - `orbic::component()` 및 UI 뷰 `#[component]`와의 풀스택 심볼 충돌을 없애기 위해 프론트 기능 조립 빌더는 **`feature()` (`UiFeature`)**로 고정합니다(`.provide()`, `.wrap()`, `.route()`).
- **타깃 특화 기능 및 저수준 드로잉 탈출구**:
  - **Web 전용 확장**: DOM 시맨틱 속성(`web::class`, `web::aria_label`), History/Storage/DOM 이벤트, Ionic 웹 컴포넌트.
  - **Native 전용 확장 및 커스텀 드로잉**: OS 창/메뉴 제어, 시스템 단축키, 그리고 크리에이티브 툴(뷰포트·타임라인·노드 에디터·히스토그램)에서 직접 GPU/CPU 패스를 발행하는 `Canvas` / `paint(bounds, ctx) => { ... }` 탈출구.

### 컴포넌트 인자 규칙 (확정, 2026-11-24)

`#[component]` 함수의 인자는 다음 세 출처 중 하나에서만 옵니다.

1. **주입**: `Ctx<T>`는 `feature().provide()`가 제공한 값입니다. 제공한 값은 그 기능과 하위 조립에서만 보이고, 조립 시점에 해석합니다.
2. **라우트 파라미터**: 라우트로 진입하는 컴포넌트는 경로·쿼리를 `hyve` 모델로 파싱한 값을 `Path<T>`와 `Query<T>`로 받습니다. 경로 자리표시자는 `.route("/projects/{id}", ...)`처럼 `{이름}`으로 쓰고 모델의 같은 이름 필드에 채웁니다(`orbic` 핸들러와 같은 타입, `hyve/docs/02-architecture.md` §2.4).
3. **프로퍼티**: 그 밖의 인자는 부모의 `ui!`가 넘깁니다.

`.route()`로 진입하는 컴포넌트는 앞의 두 가지만 받을 수 있습니다. 타입이 같다는 이유로 인자를 암묵적으로 만들거나 공유하지 않습니다.

### 문법 세부 규칙 (제안, 2026-11-24)

- **위젯 이름**: 스코프의 Rust 항목으로 해석합니다. 내장 위젯과 사용자의 `#[component]` 함수가 같은 방식으로 태그가 되고, 알 수 없는 위젯·프로퍼티·`web::*`/`native::*` 속성은 컴파일 타임 진단이 됩니다.
- **DSL이 해석하는 자리**: `"{expr}"` 보간과 `16px` 같은 단위 리터럴은 텍스트 리프와 프로퍼티 값 자리에서만 해석합니다. 이벤트 핸들러 본문과 제어문의 조건식은 수정 없이 그대로 Rust로 전달합니다.
- **조건과 반복**: `if`와 `for`는 반응형 영역입니다. 조건이나 목록이 의존하는 시그널이 바뀌면 그 영역만 갱신합니다. `for`의 `key`는 필수이며 빠지면 컴파일 타임 진단이 됩니다.
- **비동기 핸들러**: `=> async move { ... }`는 뷰 스코프에 묶인 작업으로 실행되어 UI 스레드를 막지 않고, 뷰가 언마운트되면 취소됩니다.
- **문법 오류**: 어떤 입력에서도 유한한 시간 안에 스팬이 붙은 진단으로 끝납니다.
  - **구현 세부 (제안, 2026-11-25)**: 오타 난 프로퍼티·이벤트·`web::` 속성은 컴파일러의 "비슷한 이름" 제안이 그대로 `ui!` 문법이 되도록(이벤트 `clicke =>` → `clicked =>`) 만듭니다. 타입이 안 맞는 값은 "이 프로퍼티는 `f32`를 받음", 자식을 받지 않는 위젯은 "자식을 받지 않음", 빠진 필수 프로퍼티는 그 이름, 모르는 단위는 지원 단위(`px`·`rem`·`%`)를 알려 줍니다. `2em`처럼 Rust 렉서가 먼저 거부하는 토큰은 렉서의 오류가 나갑니다.
- **프로퍼티가 아닌 인자**: 사용자 `#[component]`는 다음 규칙으로 호출부 문법을 받습니다.
  - 자식 블록: 이름이 `children`이고 타입이 `Element`인 인자. 없는 컴포넌트에 자식을 쓰면 진단입니다.
  - 이벤트: 타입이 `Callback<Args>`인 인자. 호출부에서는 `이름(args) => { ... }`로 넘깁니다.
  - 생략: `Option<T>` 인자는 생략할 수 있고, 그 밖의 프로퍼티가 빠지면 진단입니다.
  - 위치 텍스트 리프는 내장 텍스트 위젯(`Text` 등)에만 씁니다. 사용자 컴포넌트는 이름 있는 프로퍼티로 받습니다.
- **타깃 구분**: Web DOM과 Native는 Cargo 피처가 아니라 컴파일 타깃으로 갈립니다. `wasm32` 타깃이면 Web DOM, 그 밖이면 Native입니다. `web::*`와 `native::*` 속성은 두 타깃 모두에서 이름과 타입을 검증하고, 해당하지 않는 타깃에서는 코드를 내지 않습니다. 그래서 같은 화면 코드에 두 네임스페이스를 함께 써도 됩니다.
- **웹의 `Canvas`**: Phase 1의 Web DOM 타깃은 `paint(bounds, ctx)`의 `PaintCtx` 명령을 `<canvas>` 2D 컨텍스트에 그립니다. WebGPU 경로는 외부 GPU 텍스처 규격(`04-open-questions.md` #1)과 함께 정합니다.

### 앱 루트 (제안, 2026-11-24)

- `orbic`의 `OrbicApp`과 대칭으로, UI 앱의 루트는 `SergeApp`입니다. `SergeApp::new()`에 `.provide()`로 앱 스코프 값을, `.feature()`로 `UiFeature`를 등록하고 `.run()`으로 실행합니다. `UiFeature`도 `.feature()`로 하위 기능을 가질 수 있습니다.
- `.run()`은 컴파일 타깃에 따라 Web DOM에서는 문서에 마운트하고, Native에서는 창을 엽니다. 라우트가 없는 앱은 `.root(Component)`로 첫 화면을 지정합니다.
- 원격 호출에 쓸 전송은 앱 루트에서 `.provide()`로 제공합니다(`03-adapters-and-pipeline.md` §2, `hyve/docs/03-adapters-and-pipeline.md` §3.2).
- `RequireAuth`가 읽는 것은 `serge-router`가 정의하는 `AuthState`(인증 여부를 담은 시그널)입니다. 앱이 루트에서 `.provide()`로 제공하고 로그인·로그아웃 때 갱신합니다. 제공하지 않고 `RequireAuth`를 쓰면 조립 시점 오류입니다. 화면 가드는 진입을 막거나 이동시킬 뿐이며, 서버의 권한 검사를 대신하지 않습니다.
- **구현 세부 (제안, 2026-11-25)**: 전송은 타입으로 찾는 `.provide()` 대신 `.transport(Arc<dyn Transport>)`로 제공합니다(`hyve_api::provide_transport`와 같음). `.layout(|screen| ui! { .. })`는 모든 화면을 감싸는 틀(내비게이션 바 등)을 마운트 때 한 번 만들고, `location()`은 현재 주소를 추적되는 값으로 돌려줍니다. 조립 오류(라우트 중복, 잘못된 자리표시자, `AuthState` 없는 `RequireAuth`, 라우트도 `.root()`도 없음)는 `.assemble()`이 모아서 돌려주고 `.run()`은 그 목록으로 멈춥니다. 시작 경로·`.provide()`·기능 스코프 수명은 `04-open-questions.md` 결정 #3을 따릅니다.

### 구현 규칙 (제안, 2026-11-25)

1차 프로토타입을 구현하며 정한 규칙입니다.

- **클로저 규칙**: `ui!`가 만드는 클로저(프로퍼티 식, 영역, 핸들러, `paint`)는 모두 `FnOnce + Clone`이고 실행할 때마다 새 복제본을 부릅니다. 캡처하는 값은 `Clone`이어야 하고(`Signal<T>`·`Ctx<T>`는 싼 핸들), 실행 사이에 남는 상태는 시그널에 둡니다. 두 자식 클로저가 같은 비`Copy` 값을 쓰면 Rust의 이동 규칙대로 먼저 복제합니다.
- **프로퍼티 값**: 리터럴, 단위 리터럴(`16px`, `1.5rem`, `50%`), 경로, 필드 접근, 부호 반전, 범위, 튜플·배열은 한 번 평가하는 고정 값이고, 그 밖의 식은 읽은 시그널이 바뀌면 다시 평가합니다. `{expr[:spec]}`가 든 문자열 리터럴은 다시 평가하는 `format!`입니다. 접미사 없는 숫자 리터럴은 프로퍼티의 타입을 따릅니다(`font_weight: 700`은 `u16`, `grow: 1.0`은 `f32`). `em` 단위는 없습니다. 고정 값 중 소스에 적힌 리터럴만으로 된 값(`16px`, `"Save"`, `true`, `0.0..=1.0`)은 그 자리에서 만든 모든 위젯에 같으므로 웹 타깃의 템플릿에 들어가고, 경로·필드 접근(`padding: gap`)은 위젯마다 다를 수 있어 복제본마다 적용합니다.
- **정렬 값**: `align`·`justify`는 `Align::Center`처럼 씁니다. prelude는 문서 예시의 `v_stack`·`h_stack`만 소문자 상수로 내보냅니다(`start`·`end` 같은 소문자 상수는 그 prelude를 쓰는 파일의 `let start = ..`를 상수 패턴으로 바꿔 버림).
- **사용자 컴포넌트**: `#[component]`는 함수를 그대로 두고 같은 이름의 타입 상태(typestate) 빌더를 만듭니다. 필수 프로퍼티·이벤트가 빠지면 그 이름을 적은 컴파일 오류입니다. 컴포넌트 호출은 영역이라 프로퍼티 식이 읽은 시그널이 바뀌면 다시 만들어지고(지역 상태는 사라짐), 본문은 추적하지 않습니다.
- **키 있는 목록**: 같은 키의 항목은 값이 같으면 뷰를 유지한 채 옮기고, 값이 바뀌면 그 항목만 다시 만듭니다(항목 타입은 `Clone + PartialEq`). 같은 키가 두 번 나오면 서로 다른 항목으로 다룹니다.
- **뷰 스코프**: 뷰는 `hyve` `Scope`와 반응형 `Owner`의 짝입니다. 효과·핸들러·작업은 뷰의 스코프 안에서 실행되고, 뷰가 언마운트되면 그 아래의 작업과 `#[server]` 호출이 취소됩니다. `#[server]` 호출은 만들어질 때 스코프에서 전송을 찾습니다.
- **UI 작업**: `spawn(future)`는 현재 뷰에 묶인 작업, `sleep(duration)`은 두 타깃 공용 타이머입니다. 네이티브는 UI 스레드에서 작업을 폴링하되 `serge`의 `tokio` 런타임 문맥 안에서 폴링하므로 인프로세스 `#[server]` 핸들러가 I/O와 타이머를 쓸 수 있습니다.
- **`FrameSource`**: `paint` 안에서 읽은 프레임 소스(`SharedRingBuf` 등)가 바뀌면 다음 프레임에 그 캔버스만 다시 그립니다. 시그널을 샘플마다 바꾸지 않습니다.
- **`PaintCtx`**: 두 타깃 공용의 구체 타입 하나입니다. 예시의 `draw_histogram`·`draw_sparkline`은 앱이 정의하는 확장 트레이트입니다.
- **헤드리스 테스트**: `serge::testing::Headless`가 네이티브 런타임을 창 없이 돌리고(클릭·입력·키·슬라이더 시뮬레이션, 작업 실행), `CpuRenderer`가 그 장면을 픽셀로 그립니다. 웹은 `serge::testing::mount`로 실제 DOM에 마운트해 `wasm-bindgen-test`로 확인합니다.

### 확정 문법 기준 예시 (컴포넌트 조립 · `ui!` · 인라인 서버 함수 · Shm Ring Buffer · Live Activity)

```rust
use hyve::prelude::*;
use serge::prelude::*;

// 1) 도메인/기능 단위 조립: `serge::feature()` 순수 Rust 체이닝 고정 (`orbic::component()`와 충돌 0)
pub fn project_feature() -> UiFeature {
    feature()
        .provide(|| ProjectDraftStore { title: signal(String::new()), exposure: signal(0.0_f32) })
        .wrap(RequireAuth::redirect("/login"))
        .route("/projects", ProjectEditorCard)
}

// 2) 뷰(UI 트리) 마크업: `ui!` 5대 최소 문법 + 인라인 `#[server]` 호출 (Web DOM & Native GPU/CPU 공통)
#[component]
pub fn ProjectEditorCard(store: Ctx<ProjectDraftStore>) -> Element {
    // 라우트 진입 컴포넌트는 주입(`Ctx<T>`)과 라우트 파라미터만 받는다. `Signal<T>`는 `Copy` 핸들이라 꺼내 써도 된다.
    let exposure = store.exposure;
    let liked = signal(false);

    ui! {
        Card {
            layout: v_stack,
            gap: 8px,
            padding: 12px,
            Text { "Exposure: {exposure():.2}" }
            Slider {
                range: -5.0..=5.0,
                value: exposure(),
                changed(v) => { exposure.set(v); }
            }
            Button {
                text: if liked() { "좋아요 취소" } else { "좋아요" },
                clicked => { liked.toggle(); }
            }
            Button {
                text: "저장",
                clicked => async move {
                    // `#[server]` 함수 호출: 클라이언트 빌드에서는 서버 본문·DB 의존성이 완전히 제거된 RPC 스텁으로 치환됨
                    let _ = create_project_rpc(CreateProject { title: store.title.get() }).await;
                }
            }
            // 저수준 GPU/CPU 또는 Canvas/WebGPU 직접 드로잉이 필요할 때는 Canvas 위젯의 paint 이벤트로 탈출
            Canvas {
                height: 64px,
                paint(bounds, ctx) => {
                    ctx.draw_histogram(bounds, exposure());
                }
            }
        }
    }
}

// 3) 로컬 엔진 공유 메모리 Ring Buffer(`SharedRingBuf`, #[repr(C)] POD) & iOS Dynamic Island(`#[ios::live_activity]`) 활용
#[component]
pub fn RealtimeTelemetry(ring_buf: SharedRingBuf<FrameMetrics>) -> Element {
    ui! {
        Canvas {
            height: 80px,
            paint(bounds, ctx) => {
                // 인프로세스든 로컬 데몬(Shm IPC)이든 찢어짐(tearing) 없는 스냅샷/읽기 가드로 최신 샘플을 안전하게 드로잉
                let mut samples = [FrameMetrics::ZERO; 128];
                let n = ring_buf.read_latest_snapshot(&mut samples);
                ctx.draw_sparkline(bounds, &samples[..n]);
            }
        }
    }
}

// Apple ActivityKit 제약상 런타임 GPU 렌더링이 불가하므로,
// 정적 ui! 서브셋(Canvas·실시간 signal 제외)을 `hyve build` 시점에 SwiftUI Widget Extension으로 트랜스파일(codegen)함
#[ios::live_activity]
pub fn ExportIsland(state: ExportStatus) -> LiveActivity {
    live_activity! {
        compact_leading: ui! { Icon { "cube" } },
        compact_trailing: ui! { Text { "{(state.progress * 100.0) as u32}%" } },
        expanded: ui! {
            Stack { gap: 8px,
                Text { "{state.project_name} 내보내는 중..." }
                ProgressBar { value: state.progress }
            }
        }
    }
}
```

위 예제의 `#[ios::live_activity]`와 `#[android::widget]`는 정적 뷰를 플랫폼 코드로 컴파일합니다. `[package.metadata.serge]`에 `widgets = ["src/platform_widgets.rs"]`와 Android 라이브러리의 `android_package`를 선언하면 `hyve build`가 생성합니다. 결과는 `target/serge-widgets/<패키지>/current`에 기록된 세대 디렉터리에 있습니다. iOS 디렉터리는 ActivityAttributes·SwiftUI 화면·WidgetBundle 진입점을 담습니다. Android 디렉터리는 Glance 위젯·수신기·매니페스트·리소스를 담은 Gradle 라이브러리입니다. 호스트는 선언 함수를 호출해 얻은 `PlatformWidget.state_json`을 전달합니다. JSON에는 뷰에서 참조하는 필드만 실립니다. [실행 예제](../apps/serge-playground/src/platform_widgets.rs) · [코드 생성 테스트](../apps/serge-cli/tests/widgets.rs)

> 위 `ui!` 5대 최소 문법과 `feature()`/`#[component]` 분리는 확정 규격이며, 잔여 타깃 확장 세부 사항은 [`04-open-questions.md`](04-open-questions.md)에서 추적합니다.

## 2. `hyve` CLI와 통합된 멀티타깃 개발 흐름

- **내장 웹 데브 서버 (`serge-web-dev` / `hyve dev --web`)**: CRA(Create React App)나 Vite처럼 `orbic` 백엔드 없이도 즉시 로컬 개발 서버를 띄워 WASM/JS 번들링, 정적 에셋 서빙, SPA 라우팅 폴백, 상태 보존 HMR을 수행합니다.
  - **Phase 1 프로토타입 (2026-11-25)**: 소스가 바뀌면 다시 빌드하고 페이지를 새로 고칩니다(상태 보존 HMR은 **미완**). 웹 앱은 매니페스트의 `[package.metadata.serge] web = true`로 표시하고, 번들은 `target/serge/<패키지>/`에 생깁니다. 앱의 `web/` 폴더(있으면 `index.html` 포함)를 번들에 복사합니다.
- **네이티브 앱 로컬 실행**. `hyve dev --native`는 데스크톱 앱을 빌드해 창을 엽니다. `hyve-state`를 보존하는 네이티브 핫리로드와 모바일 타깃 선택(`--ios`, `--android`)은 후속 단계의 목표입니다.
- **통합 정적 검증 (`hyve lint`)**: 웹 접근성(a11y)·Hydration 불일치 검사와 네이티브 UI 스레드 블로킹·WebView 무단 유입·레이아웃 순환 참조 검사를 함께 수행합니다.
- **헤드리스 통합 테스트 (`hyve test`)**: 웹 타깃은 헤드리스 DOM/SSR 스냅샷으로, 네이티브 타깃은 **CPU 폴백 렌더러(`serge-native-cpu`)**를 이용한 프레임 버퍼 스냅샷 및 입력 시뮬레이션으로 GPU 없는 CI에서도 고속 검증합니다.

### 개발 세션·데스크톱 실행 기록 (2026-11-26)

`hyve dev --web`과 `hyve dev --native`에서 키와 타입을 지정한 `dev_signal`의 값을 재빌드 뒤 복원합니다. 웹에서는 탭 단위로 보관하고 네이티브에서는 개발 런처가 세션을 관리합니다. 잘못된 빌드를 고치면 마지막 성공한 앱의 값을 이어갑니다. 여러 창에 공유할 값은 `hyve::state::shared_signal`을 사용합니다. 워커가 변경하면 각 창의 UI 스레드가 해당 화면의 구독자를 실행합니다. [실제 재빌드 테스트](../apps/serge-cli/tests/reload.py) · [다중 창 CPU·Metal 테스트](../apps/serge-playground/tests/windows.rs)

`serge/desktop-system`으로 네이티브 메뉴·트레이를 활성화할 수 있습니다. macOS에서는 메뉴 막대, 컨텍스트 메뉴와 상태 항목의 명령이 포커스를 가진 창의 뷰에 전달됩니다. Linux X11 창은 네이티브 GTK 메뉴 막대와 우클릭 메뉴를 열고, 메뉴 명령을 포커스한 창으로 전달합니다. Wayland 세션에서는 XWayland로 GTK 메뉴와 웹뷰를 열 수 있습니다. 순수 Wayland 창은 데스크톱 셸의 D-Bus 상태 항목을 사용합니다. 버튼·체크박스·입력·슬라이더는 AccessKit을 통해 상태와 액션을 노출합니다. [AppKit 명령 테스트](../apps/serge-playground/tests/system_menu.rs) · [접근성 테스트](../crates/serge/tests/native.rs)

### 적응형 웹 탐색과 모달

`serge/ionic` 피처의 `ionic::shell(navigation, content)`는 같은 탐색 요소를 넓은 화면에서는 측면에, 좁은 화면에서는 하단에 배치합니다. `ionic::sheet(label, dismiss_label, content, on_dismiss)`를 조건부 영역에 넣으면 모달 안에서 포커스가 순환하고 닫은 뒤 열었던 컨트롤로 돌아갑니다. Escape, 배경 클릭과 손잡이의 아래쪽 스와이프가 같은 닫기 콜백을 호출합니다. SSR 호스트는 `ionic::STYLESHEET`을 문서 head에 포함할 수 있습니다. [컴포넌트](../crates/serge-web-ionic/src/lib.rs) · [Chrome 인수 테스트](../crates/serge/tests/web.rs)

### Android 앱 실행과 인수

`hyve init`의 UI·풀스택 템플릿은 Android NativeActivity 진입점을 포함합니다. 메인 활동이 제공한 `AndroidApp`을 `serge::android::launch`에 넘기면 같은 라우트와 화면을 실행합니다. 터치와 키보드 포커스는 네이티브 트리를 사용하고 AccessKit이 Android 접근성 트리와 액션을 연결합니다. 앱이 배경으로 가면 렌더 표면을 해제하고 복귀 시 같은 뷰 상태로 다시 그립니다. Vulkan 소프트웨어 어댑터만 제공하는 Android 환경에서는 CPU로 그립니다. [Android NativeActivity](https://developer.android.com/reference/android/app/NativeActivity)

`serge-playground`는 기본적으로 앱의 `127.0.0.1:3000/v1`에 접속합니다. 개발 에뮬레이터에서 호스트 서버를 쓸 때는 `adb reverse tcp:3000 tcp:3000`으로 연결합니다. 다른 주소는 빌드 시 `SERGE_API_URL`로 지정합니다. 인수 테스트는 선택한 에뮬레이터에서만 임시 서버와 포트를 사용해 APK 설치, 카운터 상태 복원, 로그인·프로젝트 입력·저장·재조회, 세션 만료 후 쓰기 거부와 DB 행 수를 확인합니다. Android 35 arm64에서 APK와 AAB로부터 생성한 APK 양쪽에서 수행했습니다. [에뮬레이터 통합 테스트](../apps/serge-playground/tests/android/projects.py)
