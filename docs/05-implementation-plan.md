# 05. 구현 계획 (serge)

> 상태: **제안** · 기준일: 2026-11-25

Phase 1(`v0.1.0`)의 작업 순서와 완료 기준입니다. Phase 1에 들어가는 크레이트는 [`04-open-questions.md`](04-open-questions.md)의 Phase 컷이, 크레이트 구조와 책임은 [`02-architecture.md`](02-architecture.md)가 정합니다. 패밀리 차원의 완료 기준은 `hyperish/docs`의 `00-vision.md`에 있습니다.

`serge`는 `hyve`의 `0.1.0-alpha.N` 프리릴리스가 배포된 뒤 그 버전에 의존해 시작합니다(`hyperish/docs` `05-development-rules.md` §5). 7단계의 원격 호출은 `orbic`의 프리릴리스가 있어야 끝까지 검증할 수 있고, `serge` 크레이트는 `orbic`에 의존하지 않으므로 그 검증은 `apps/serge-playground`에서 합니다.

## 1. 출발점

- (2026-11-24 기준) 코드는 서드파티 의존성이 없는 자리표시자였고, 2026-11-25에 아래 §4의 1차 프로토타입으로 교체했습니다. `Cargo.lock`을 커밋하고, CI는 `hyve`·`orbic`을 형제 경로로 체크아웃해 포맷·린트·테스트(`server` 피처 포함)·헤드리스 Chrome DOM 테스트·최소 Rust 버전·macOS 데스크톱 테스트를 돌립니다(`.github/workflows/ci.yml`).
- 공개 API의 기준은 문서입니다. `crates/serge/tests/mvp.rs`는 문서 예시를 옮긴 테스트로 다시 씁니다.
- 구현에 앞서 할 일:
  1. 포크 대상의 기준 리비전을 다시 확인해 고정하고 기록합니다(`hyperish/docs` `05-development-rules.md` §7).
  2. 워크스페이스를 `02-architecture.md` §1의 크레이트 구조로 재편합니다.
  3. `hyve` 의존(쇼케이스 앱은 `orbic`도)을 프리릴리스의 정확한 버전 요구로 바꾸고, `Cargo.lock`을 커밋하고, CI에 최소 Rust 버전 빌드를 추가합니다.

## 2. 작업 순서

| 단계 | 크레이트 | 산출물 | 앞선 단계 |
|---|---|---|---|
| 1 | `serge-core` | 뷰 트리, 컴포넌트 인자 규칙, 이벤트 흐름, `ScenePrimitive`·`PaintCtx`, `feature()` 조립, `SergeApp` 루트 | 없음 |
| 2 | `serge-macros` | `ui!` 5대 문법과 세부 규칙, `#[component]` | 1 |
| 3 | `serge-style` | 디자인 토큰, 스타일·레이아웃 IR | 1 |
| 4 | `serge-router` | 타입 안전 라우트, 가드, 화면 전환 시 이전 요청 취소 | 1 |
| 5 | `serge-web-dom`, `serge-web-dev` | `tachys` 포크 편입, 템플릿 복제와 패칭, 조건·반복 영역 갱신, 로컬 데브 서버 | 1~4 |
| 6 | `serge-native-layout`, `serge-native-text`, `serge-native-cpu`, `serge-native-gpu`, `serge-platform-desktop` | `taffy`·`parley`/`swash`·`vello_cpu`·`wgpu` 포크 편입(Phase 1의 GPU 경로는 `wgpu` 인스턴스 배칭이며 `vello` 편입은 Phase 2 이후), 창과 입력, GPU → CPU 폴백 정책 | 1~4 |
| 7 | `serge-adapter-client` | 인프로세스 연결, 원격 전송(두 레일, 스키마 불일치 폴백, 자격 증명 부착, 언마운트 시 취소) | 1 |
| 8 | `serge`, `apps/serge-playground`, `hyve-serge` | 재수출과 prelude, 두 타깃을 보여 주는 쇼케이스, `hyve`가 호출하는 도구 단계 | 1~7 |

SSR/SSG, 적응형 웹 UI, 영속 시그널, 데스크톱 시스템 API, 공유 메모리 IPC는 Phase 2이고, 모바일과 위젯 코드젠, 옵트인 웹뷰, 실시간 스트림은 Phase 3입니다.

## 3. 완료 기준

- **문서 예시**: `README.md`의 `CounterCard`와 `01-dx-and-dsl.md` 예시의 `project_feature`·`ProjectEditorCard`·`RealtimeTelemetry`가 Web DOM과 Desktop Native에서 동작합니다. `#[ios::live_activity]` 예시는 Phase 3이라 제외합니다.
- **문법**: 알 수 없는 위젯·프로퍼티·네임스페이스, 빠진 `key`, 깨진 문법에 대한 컴파일 실패 테스트가 있고, 모두 유한한 시간 안에 진단으로 끝납니다. 핸들러 본문에 쓴 일반 Rust 코드가 그대로 컴파일됩니다.
- **반응성**: 마운트한 뒤 시그널을 바꾸면 텍스트, 프로퍼티, `if`/`for` 영역이 갱신됩니다. 웹은 `wasm32` 번들을 헤드리스 브라우저에서 실행해, 네이티브는 CPU 경로의 프레임 버퍼 스냅샷으로 확인합니다. 헤드리스 브라우저 러너는 구현이 고르고 외부 의존성 목록에 기록합니다.
- **렌더러 대칭**: 같은 `ScenePrimitive`를 GPU와 CPU 경로가 모두 그리고(두 경로의 결과는 구현이 정한 허용 오차 안에서 일치), GPU가 없는 CI에서 CPU 경로로 테스트가 통과합니다. 폴백 정책(자동 전환, 명시 지정, 명시한 GPU 실패 시 오류)을 테스트로 확인합니다.
- **비동기**: 핸들러의 비동기 작업이 UI 스레드를 막지 않고, 뷰를 언마운트하면 취소됩니다.
- **원격 호출**: `apps/serge-playground`에서 `#[server]` 호출이 인프로세스 전송과 원격 전송 양쪽으로 동작하고, 두 레일에서 같은 결과와 같은 오류를 내며, 스키마가 어긋나면 표준 레일로 내려갑니다.
- **의존 경계**: 네이티브 크레이트의 의존 그래프에 WebView가 없고, 웹 번들에 네이티브 레이아웃 솔버가 들어가지 않습니다.
- **포크 검증**: 포크본이 원본과 달라진 점과 측정 결과를 남깁니다.
- **CI**: 포맷, 린트, 테스트, 최소 Rust 버전 빌드가 통과하고, 공개 크레이트 전부가 패키징됩니다. 충족하면 `0.1.0-alpha.N` 프리릴리스를 배포합니다.

## 4. 진행 상황 — 1차 사용성 프로토타입 (2026-11-25)

목표는 Phase 1 완료가 아니라 "손으로 써 보고 사용성을 판단할 수 있는" 프로토타입입니다. 웹(실제 DOM)과 데스크톱(창, GPU 렌더러와 CPU 폴백)에서 같은 앱이 돌고, `#[server]` 호출이 `orbic` 백엔드로 갑니다. 써 보는 방법은 `README.md`에 있습니다.

| 단계 | 상태 | 확인 |
|---|---|---|
| 1 `serge-core` | 구현 | 뷰 트리, 위젯, 뷰 스코프·작업·취소, `PaintCtx`·`ScenePrimitive`, `FromView`(`Ctx`·`Path`·`Query`) |
| 2 `serge-macros` | 구현 | 문서 예시가 그대로 컴파일(`crates/serge/tests/macros.rs`). 오용 16가지(알 수 없는 위젯·프로퍼티·이벤트·네임스페이스·`web::` 속성·단위, 빠진 `key`·필수 프로퍼티, 자식을 받지 않는 컴포넌트, 타입 불일치, 깨진 값·`if`·`for`·보간·토큰)가 스팬이 붙은 오류로 끝남(`crates/serge/tests/diagnostics.rs`, 2026-11-25) |
| 3 `serge-style` | 구현 | 토큰(밝은·어두운 테마), 스타일 IR, CSS 출력 |
| 4 `serge-router` | 구현 | 가드 이동, 화면 전환 시 진행 중 호출 취소, 기능 스코프 유지, 조립 오류(`crates/serge/tests/app.rs`), 프로퍼티가 시그널을 읽는 컴포넌트의 재생성(`crates/serge/tests/native.rs`) |
| 5 `serge-web-dom`, `serge-web-dev` | 구현, HMR **미완** | 헤드리스 Chrome에서 DOM 테스트(`crates/serge/tests/web.rs`), 데브 서버 테스트, 실제 브라우저로 쇼케이스 전체 흐름(링크·뒤로 가기·입력·`#[server]`·캔버스·라이브 리로드) 확인. 템플릿 복제(2026-11-25): 자리마다 템플릿 하나, 목록 항목은 원소 생성 없이 복제되고 복제본마다 텍스트·위젯별 값·입력 상태·핸들러가 따로 적용됨(`list_items_are_cloned_from_templates`, `per_instance_values_are_not_baked`), 실제 브라우저 흐름 재확인 |
| 6 네이티브 | CPU·GPU 경로 구현 | 헤드리스 런타임 테스트, 1×·2× CPU 스냅샷, 쇼케이스 전 화면 렌더. GPU 경로(2026-11-25): 모든 프리미티브·반투명·중첩 클립·글자 장면과 쇼케이스 6개 화면을 1×·2×로 GPU와 CPU에서 그려 §1.2 허용 오차 안(쇼케이스는 평균 차 0.03 이하, 32 초과 픽셀 0.006% 이하), 프레임당 드로우콜 1개(`crates/serge-native-gpu/tests/parity.rs`, `apps/serge-playground/tests/backend.rs`). 폴백 정책을 실제 디바이스 생성으로 확인(빌드에 없는 백엔드로 실패시킨 명시 `gpu`는 오류, 자동은 CPU). 실제 창을 열어 CPU 프레임과 GPU 표면·디바이스 생성까지 확인했으나, 화면에 표시된 GPU 픽셀과 화면 캡처·실제 입력은 **미검증**(실행 환경의 화면이 잠겨 창이 가려진 상태로 보고되고, 화면 기록·손쉬운 사용 권한이 없음). 120fps는 측정하지 않음(**미검증**) |
| 7 `serge-adapter-client` | 구현, 락 없는 링·TLS **미완** | 루프백 `orbic::serve`에 원격 호출: 두 레일과 스키마 불일치 폴백에서 인프로세스와 같은 결과·같은 오류, 스코프 해제 시 취소(`apps/serge-playground/tests/backend.rs`). 웹은 뷰를 언마운트하면 진행 중 `fetch`가 중단됨(`crates/serge/tests/web.rs`) |
| 8 `serge`, 쇼케이스, `hyve-serge` | 구현 | `hyve-serge stages`·`build`·`preview`, `hyve build --debug`가 실제 `hyve-serge`를 거쳐 웹 번들을 만듦. 의존 경계 테스트(`crates/serge/tests/deps.rs`) |

§3 완료 기준 중 남은 것: 포크 검증(아래 기록은 달라진 점까지, 소스 편입과 원본 대비 측정은 **미완**), 패키징(`hyve`·`orbic` 프리릴리스 뒤), 상태 보존 HMR(Phase 2). CI는 `feat/**` 브랜치 push에서 돌고, 리눅스는 소프트웨어 Vulkan(Mesa lavapipe), macOS는 Metal로 GPU 비교를 건너뛰지 않고 실행합니다(`HYPERISH_TEST_GPU=require`).

### 포크 기록 (2026-11-25)

`hyperish/docs` `05-development-rules.md` §7의 8번 규칙에 따라, 소스를 편입하지 않은 포크는 검증을 충족한 것으로 보지 않습니다. 아래는 달라진 점의 기록이고, 원본 대비 측정은 **미완**입니다.

| 대상 | 원본 | 달라진 점 |
|---|---|---|
| `taffy` | `0.15.1` 고정 의존 | 원본 그대로. 피처는 `std`·`flexbox`·`block_layout`·`taffy_tree` |
| `parley`, `fontique` | `0.13.0` 고정 의존 | 원본 그대로. Linux에서는 fontconfig를 실행 시점에 불러옴(`fontconfig-dlopen`) |
| `vello_cpu` | `0.4.0` 고정 의존 | 원본 그대로. 피처는 `std`·`text`·`u8_pipeline`. GPU 경로의 글리프·경로 마스크도 이것으로 만듦(`swash` 대신) |
| `wgpu` | `30.0.2` 고정 의존 | 원본 그대로. 피처는 `std`·`wgsl`·`metal`·`vulkan`·`dx12` |
| `winit` | `0.30.13` 고정 의존 | 원본 그대로(표의 `0.31.0-beta.4` 대신 안정판) |
| `serge-web-dom` | `tachys` `0.3.0-rc1` 레퍼런스 | 자체 구현. 같은 점: 실제 DOM의 세밀 갱신, 템플릿 복제 뒤 동적 지점만 채움. 다른 점: `reactive_graph` 대신 `hyve-state`, 제네릭 튜플 뷰 타입 대신 런타임 위젯 트리와 자리(`Site`)별 템플릿, 골격을 컴파일 타임 문자열이 아니라 첫 마운트 때 만듦 |
| `serge-native-gpu` | GPUI 아키텍처 레퍼런스(포크 금지) | 자체 구현. 같은 점: 인스턴스 버퍼 배칭, 글리프 아틀라스. 다른 점: 프레임당 파이프라인·드로우콜 하나(클립을 셰이더 커버리지로), 경로를 GPU 래스터화 대신 CPU 마스크로, 그림자·외부 텍스처 프리미티브 없음 |
