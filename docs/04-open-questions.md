# 04. 결정 기록 (serge)

> 기준일 2026-11-26 · 결정은 **확정**이며, 이전 기록의 제안과 구현 상태는 각 항목에 표시합니다.

`serge`의 설계 결정과 그 근거를 추적합니다.

## 열려 있는 결정

현재 0건입니다.

## 결정됨

- **#1 · 외부 뷰포트** (2026-11-26). 호스트가 소유한 디바이스와 프레임 타깃, CPU 픽셀 대체, 웹 캔버스의 수명·입력·색 형식을 정했습니다. [뷰포트 v1](03-adapters-and-pipeline.md#viewport-v1)
- **#2 · 프로덕션 서빙과 옵트인 웹뷰** (2026-11-26). 정적 번들 파일 형식과 라우트 우선순위, 취소 가능한 인프로세스 SSR 시그니처, 웹뷰 이벤트의 순서·중복·리로드 복구 조건을 확정했습니다. [번들 v1](03-adapters-and-pipeline.md#bundle-v1) · [SSR v1](03-adapters-and-pipeline.md#ssr-v1) · [웹뷰 v1](03-adapters-and-pipeline.md#webview-v1)

- **웹(DOM)과 네이티브(GPU+CPU 폴백) UI를 `serge` 단일 레포로 통합** (2026-11-18): `00-overview.md`
- **UI 뷰 마크업은 `ui!` proc macro DSL 5대 최소 문법으로 고정하고, 기능·라우트·가드 조립은 `serge::feature()` Rust 체이닝으로 고정** (2026-11-18 ~ 2026-11-19): `01-dx-and-dsl.md` (`orbic::component()`·`#[component]` 심볼 충돌 해소 및 `web::*`/`native::*` 네임스페이스 확정 포함)
- **Web 타깃(DOM 출력 + Leptos 파이프라인 + Ionic형 UI + CRA급 로컬 데브 서버) 및 `orbic` 프로덕션 서빙 분리 확정** (2026-11-17): `00-overview.md`, `03-adapters-and-pipeline.md`
- **Native 타깃(GPUI·`anomalyco/rune` 레퍼런스 기반 GPU + 필수 CPU 폴백, 데스크톱·iOS·Android 지원, 네이티브 UI 내 WebView 절대 금지 및 옵트인 `serge-adapter-webview` 분리) 확정** (2026-11-17 ~ 2026-11-18): `00-overview.md`, `02-architecture.md`
- **공용 스타일 IR(`serge-style`)과 네이티브 전용 레이아웃 솔버(`serge-native-layout`) 분리 및 iOS Dynamic Island·위젯의 정적 `ui!` → SwiftUI/Glance 코드젠 원칙 확정** (2026-11-19): `00-overview.md`, `02-architecture.md`, `03-adapters-and-pipeline.md`
- **crates.io 비충돌 고유명 `serge`(구 `lace`) 및 순수 `MIT` 라이선스 확정** (2026-11-19): 레포명·crates.io 패키지명·Rust 크레이트명을 `serge`로 1:1 일치
- **MIT 포크 스택(`tachys`·`taffy`·`wgpu`·`vello`·`vello_cpu`·`parley`+`swash`·`accesskit`, `winit` 외부 의존성 전용), 내부 코어 개조 방향 및 Phase 1~3 우선순위 컷 확정** (2026-11-19):
  - Phase 1(`v0.1.0`): `serge-core`·`serge-macros`(`ui!` 5대 문법)·`serge-style`·`serge-router` + Web DOM(`tachys`·`serge-web-dev`) + Desktop Native(`taffy`·`wgpu`·`vello_cpu` 폴백·`parley`/`swash`·`winit`) + `serge-adapter-client`(Tier 1 인프로세스 · Tier 3 원격 요청-응답)
  - Phase 2(`v0.2.0`): `serge-web-ssr`(SSR/SSG/SEO)·`serge-web-ionic`·`persistent_signal`·Desktop 멀티 윈도우/메뉴바/트레이/`AccessKit`·`serge-adapter-client` Tier 2 Shm IPC
  - Phase 3(`v0.3.0+`): iOS/Android 풀 HW·SW SDK + Dynamic Island/WidgetKit/Glance 코드젠 + 옵트인 `serge-adapter-webview` + `serge-adapter-client` 실시간 스트림(WebTransport·WS 폴백) (`02-architecture.md` §3)
- **시그널 표기를 단일 `Signal<T>`로 통일** (2026-11-24): `SyncSignal`·"이원화 시그널" 표기를 `hyve`가 확정한 단일 `Copy` 핸들 `Signal<T>`로 바로잡음. 스레드 모드는 `hyve-state` 내부 스토리지에서 갈림 (`00-overview.md` §1, §2, `03-adapters-and-pipeline.md` §2)
- **Phase 컷에 `serge-adapter-client` 배치** (2026-11-24): Phase 1의 `#[server]` 호출이 성립하도록 Tier 1 인프로세스와 Tier 3 원격 요청-응답을 Phase 1에, Tier 2 Shm IPC를 Phase 2에, 실시간 스트림(WebTransport·WS 폴백)을 `orbic-adapter-ws`와 같은 Phase 3에 배치(위 목록에 반영)
- **Dual-Rail 폴백에서도 고속 레일은 `hyve-serde` 바이너리 유지** (2026-11-24): WebTransport가 막혀 WS로 폴백해도 같은 바이너리 프레임을 운반하며, JSON은 표준 레일 전용 (`03-adapters-and-pipeline.md` §2)
- **포크 표 보정** (2026-11-24): `parley` 라이선스 기재, `raw-window-handle` 라이선스 표기 정정(`MIT OR Apache-2.0 OR Zlib`), 표의 버전은 확인 시점 스냅샷으로 취급 (`02-architecture.md` §3)
- **GPU → CPU 폴백 정책 확정** (2026-11-24): GPU 디바이스를 만들지 못하면 자동으로 CPU 렌더러로 전환. `HYPERISH_RENDER_BACKEND`로 `cpu` 또는 `gpu`를 명시할 수 있고, `gpu`를 명시했는데 실패하면 폴백하지 않고 오류. 헤드리스 테스트는 CPU 렌더러를 씀. 실행 중 디바이스를 잃었을 때의 전환은 **제안** (`03-adapters-and-pipeline.md` §1.2)
- **컴포넌트 인자 규칙 확정 및 확정 예시 정정** (2026-11-24): 인자의 출처를 주입(`Ctx<T>`), 라우트 파라미터, 부모가 넘기는 프로퍼티로 한정. 라우트 진입 컴포넌트가 출처 없는 `Signal<f32>`를 받던 예시를 제공된 스토어에서 꺼내 쓰도록 고치고, 필드를 메서드처럼 호출하던 `store.title()`을 `store.title.get()`으로 정정 (`01-dx-and-dsl.md` §1)
- **`ui!` 문법 세부 규칙 추가(제안)** (2026-11-24): 위젯 이름의 Rust 항목 해석과 컴파일 타임 진단, DSL이 해석하는 자리의 한정, 반응형 `if`/`for`와 필수 `key`, 비동기 핸들러의 실행·취소, 문법 오류의 유한 시간 진단 (`01-dx-and-dsl.md` §1)
- **외부 의존성 기록** (2026-11-24): 웹 바인딩(`wasm-bindgen` 계열)과 매크로용 크레이트, 네이티브 비동기 I/O의 `tokio`를 포크 표 아래에 기록 (`02-architecture.md` §3)
- **Phase 1 구현 계획 문서 추가** (2026-11-24): 작업 순서와 완료 기준을 `05-implementation-plan.md`에 둠
- **앱 루트와 조립 세부 추가(제안)** (2026-11-24): `SergeApp` 루트와 `.feature()` 중첩, 타깃 구분은 컴파일 타깃(`wasm32`이면 Web DOM), `web::*`·`native::*`의 타깃별 처리, 사용자 컴포넌트의 자식·이벤트·생략 가능한 인자, 라우트 파라미터의 `Path<T>`·`Query<T>`, 웹 `Canvas`의 2D 컨텍스트 경로 (`01-dx-and-dsl.md` §1)
- **`serge-adapter-client`의 Phase 1 범위 구체화(제안)** (2026-11-24): 인프로세스 전송을 트레이트로 받아 쓰는 것, 원격 전송의 기준 URL, 자격 증명 부착, 개발 서버의 API 프록시, `hyve-serge` 실행 파일 (`03-adapters-and-pipeline.md` §2.1)
- **폴백 정책의 우선순위 보충** (2026-11-24): 명시한 환경 변수가 헤드리스 테스트의 CPU 기본값보다 우선하고, 가상 디스플레이는 디바이스 생성 실패로 다룸 (`03-adapters-and-pipeline.md` §1.2)
- **Phase 1의 GPU 경로에서 `vello` 제외를 계획에 반영** (2026-11-24): Phase 컷과 포크 표대로 Phase 1은 `wgpu` 인스턴스 배칭이며 `vello` 편입은 그 뒤 (`05-implementation-plan.md` §2)
- **2차 점검 반영(제안)** (2026-11-24): Phase 1의 네이티브 `Bearer` 토큰 보관은 앱 책임으로 하고 OS 보안 저장소 연동은 Phase 2, 원격 전송의 레일 고정 설정 추가 (`03-adapters-and-pipeline.md` §2.1)
- **앱의 첫 화면, `.provide()`의 형태, 기능 스코프의 수명 결정 (옛 #3)** (2026-11-25): 시작 경로는 `/`이고 앱 루트의 `.start_path(..)`로 바꿉니다(웹은 주소창의 경로). `.provide(|| ..)`는 생성 클로저를 받아, 기능의 라우트에 처음 들어갈 때 그 기능의 앱 수명 소유자 아래에서 한 번 실행합니다. 기능 스코프는 그때 만들어 앱이 끝날 때 해제하므로, 기능을 벗어났다 돌아와도 그 상태가 남습니다. 화면마다의 뷰는 화면을 떠나면 해제됩니다(`01-dx-and-dsl.md` §1 앱 루트, `crates/serge-router`)
- **텍스트 보간의 포맷 지정 문법 결정 (옛 #4)** (2026-11-25): Rust `format!`과 같은 규칙입니다. `::`의 일부가 아닌 마지막 최상위 `:` 뒤가 포맷 지정이고(`"{exposure():.2}"`, `"{load() * 100.0:.0}%"`), `{{`·`}}`는 중괄호 문자입니다 (`crates/serge-macros`)
- **Phase 1 프로토타입 구현 규칙(제안)** (2026-11-25): 클로저 규칙, 프로퍼티 값의 고정/반응 구분과 리터럴 타입, `Align::*` 정렬 값, 사용자 컴포넌트의 타입 상태 빌더와 재생성, 키 있는 목록의 값 비교, 뷰 스코프와 취소, `sleep`, `FrameSource`, 헤드리스 하네스 (`01-dx-and-dsl.md` §1 구현 규칙). 앱 루트의 `.transport(..)`·`.layout(..)`·`location()` (`01-dx-and-dsl.md` §1 앱 루트)
- **Phase 1 착수 방식(확정의 실행 방식을 바꿈)** (2026-11-25): 포크 없이 베이스 크레이트를 정확한 버전으로 고정해 의존, `tachys`는 레퍼런스 전용, `swash` 대신 `vello_cpu`의 글리프 경로, `winit` 안정판 `0.30.13`, `softbuffer` 추가, `syn` 2, 새 크레이트 `serge-native`, 최소 Rust 버전 `1.94` (`02-architecture.md` §1, §3.1)
- **개발 서버와 원격 전송 세부(제안)** (2026-11-25): `api_proxy` URL 형식, `RemoteTransport` 설정과 폴백·취소, 네이티브 TLS의 신뢰 모델(운영체제 검증기, 고정 루트를 주면 그것만 신뢰, 검증 끄기 없음), 웹의 `credentials: include`, `hyve-core`의 락 없는 링을 감싼 `SharedRingBuf`(샘플은 POD, 2026-11-25에 뮤텍스 구현을 교체) (`03-adapters-and-pipeline.md` §2.1). 확정 예시의 `let mut liked`를 `let liked`로 정정(`toggle()`은 `&self`라 `mut`가 경고) (`01-dx-and-dsl.md` §1)
