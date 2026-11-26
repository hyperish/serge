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
| 7 `serge-adapter-client` | 구현 | 루프백 `orbic::serve`에 원격 호출: 두 레일과 스키마 불일치 폴백에서 인프로세스와 같은 결과·같은 오류, 스코프 해제 시 취소(`apps/serge-playground/tests/backend.rs`). `https://`(2026-11-25): 테스트 CA와 IP SAN 인증서로 TLS를 받는 앞단을 거쳐 같은 결과·같은 오류·폴백·취소, 운영체제가 모르는 CA와 다른 이름의 인증서는 백엔드에 닿기 전에 거부되고 표준 레일로 다시 보내지 않음, 잘못된 PEM은 이유와 함께 실패(`apps/serge-playground/tests/tls.rs`). 공개 CA가 서명한 실제 호스트를 운영체제 검증기로 부르는 것, Windows, 모바일의 검증기 초기화는 **미검증**. 웹은 뷰를 언마운트하면 진행 중 `fetch`가 중단됨(`crates/serge/tests/web.rs`). `SharedRingBuf`는 `hyve-core`의 락 없는 링 위(2026-11-25): 쓰는 스레드와 동시에 읽어도 찢어진·순서가 뒤바뀐 샘플이 없음(`crates/serge-adapter-client/src/ring.rs`) |
| 8 `serge`, 쇼케이스, `hyve-serge` | 구현 | `hyve-serge stages`·`build`·`dev`, `hyve build --debug`가 실제 `hyve-serge`를 거쳐 웹 번들을 만듦. `hyve dev --web` 하나로 웹 데브 서버와 orbic 백엔드(`playground-server`)가 함께 뜨고, 실제 브라우저 워크스루 8단계 통과, 서버 소스 수정 시 재기동, Ctrl-C·한 단계 실패·`hyve` 강제 종료 후 남는 프로세스와 포트가 없음을 확인(2026-11-25, 임시 위치에 `cargo install --path`로 설치한 도구로도 확인. 개발 단계 이름은 2026-11-26부터 `dev`). 표준 입력이 닫히면 개발 세션이 멈추는 것은 `apps/serge-cli/tests/cli.rs`가 확인. 의존 경계 테스트(`crates/serge/tests/deps.rs`) |

2026-11-25 시점의 §3 완료 기준 중 남은 것: 포크 검증(아래 기록은 달라진 점까지, 소스 편입과 원본 대비 측정은 **미완**), 상태 보존 HMR(Phase 2). 패키징은 CI가 공개 크레이트 15개를 패키징하고 각 패키지만으로 빌드해 확인합니다(아직 배포되지 않은 `hyve` 버전은 형제 체크아웃을 `[patch.crates-io]`로 대신 씀, 2026-11-25). CI는 `feat/**` 브랜치 push에서 돌고, 리눅스는 소프트웨어 Vulkan(Mesa lavapipe), macOS는 Metal로 GPU 비교를 건너뛰지 않고 실행합니다(`HYPERISH_TEST_GPU=require`).

2026-11-26에 쇼케이스에서 프로젝트를 저장하는 통합 테스트로 패밀리의 Phase 1 완료 기준 1번을 확인했습니다. 데스크톱 테스트는 포트 없이 로그인·저장·재조회를 수행하고 저장 행과 CPU로 그린 프로젝트 이름을 확인합니다. 브라우저 테스트는 실제 Chrome에서 같은 화면을 사용하고, 세션 쿠키를 지운 뒤 두 번째 저장이 거부되는지 검사한 다음 SQLite 파일을 다시 열어 첫 행의 ID·소유자·제목을 대조합니다. [테스트 실행 방법](../README.md)

<a id="integration-checks"></a>

### 2026-11-26 플랫폼 인수

공개 패키지는 21개입니다. Android 35에서는 NativeActivity에서 실제 시스템 신뢰 저장소로 인증한 HTTPS, 권한 있는 마이크·센서의 샘플, 구독 해제, KeyStore 교체·삭제, 네이티브 다이얼로그와 UI 저장을 확인했습니다. Linux X11에서 Secret Service 자격 증명의 교체·삭제, GTK 패널, 전역 단축키와 클립보드를 확인했습니다. WKWebView와 WebKitGTK는 별도 프로세스의 로컬 문서를 사용해 JSON 및 WASM/rkyv의 재로딩 복구를 각각 검증합니다. [Android](../apps/serge-playground/tests/android/) · [시스템 웹뷰](../apps/serge-playground/tests/webview.rs)

동일한 Rust 모델에서 iOS Live Activity, 홈·잠금 화면 위젯과 읽기 App Intent, Android Glance 위젯과 진행 알림을 생성합니다. Android 35의 AppWidgetHost에서 두 번의 상태 변경을 관찰하고 NotificationManager에서 권한 거부, 진행률 교체, 취소를 확인했습니다. iOS의 실기기 전달, 서명·스토어 제출, 나머지 포크 측정은 별도 인수 항목입니다. [코드 생성](../apps/serge-cli/src/widgets.rs) · [Android 위젯 인수](../apps/serge-playground/tests/android/widgets.py)

Pixel Fold API 35에서 가상 힌지를 180°에서 90°로 변경해 WindowManager의 Flat → HalfOpened, 분리 상태 및 NDK 힌지 센서 값을 검사하고 PiP에 진입했습니다. Android Camera2의 외부 GLES 프레임 두 장과 Vulkan 계산 셰이더에서 읽은 AHardwareBuffer 두 장의 픽셀, 두 에뮬레이터 사이의 BLE GATT 서비스 발견·읽기·구독·쓰기·알림 수신·연결 해제와 가상 MIDI 장치 왕복을 확인했습니다. macOS에서는 CoreMIDI 가상 포트, 실제 Metal 장치의 IOSurface 픽셀, 시스템 PCM 버퍼의 채널 변환을 확인했습니다. iOS용 기기 API 크레이트는 UIKit 창별 대화상자·공유·클립보드·햅틱·백그라운드 실행을 포함해 시뮬레이터 타깃으로 교차 검사했습니다. Android는 등록한 딥링크로 앱을 실행해 전체 URI 전달과 유한 진동 패턴 요청을 확인했습니다. [Android SDK 인수](../apps/serge-playground/tests/android/sdk.py) · [Metal 픽셀 검사](../crates/serge-native-gpu/src/camera.rs) · [Apple 오디오](../crates/serge-native-sys/src/audio.rs)

### 후속 통합 계약의 검증 (2026-11-26)

`serge::dev_signal("counter", "value", || 0u32)`로 공개 UI 상태의 인스턴스 키와 필드 키를 지정합니다. 웹 개발 서버는 탭의 세션 저장소를 사용하고 네이티브 개발 감독기는 임시 디렉터리를 자식 프로세스와 공유합니다. 성공한 웹 빌드만 교체하며 네이티브는 빌드에 성공한 뒤 창을 다시 시작합니다. 실행 중인 앱에서 값을 변경하고 소스를 편집하는 회귀 테스트는 두 타깃에서 값 복원, 컴파일 실패 시 기존 화면 유지, 타입 변경 시 초기화를 확인합니다. [CLI 통합 테스트](../apps/serge-cli/tests/reload.py)

| 계약 | 이번에 실행한 검증 | 제품 구현의 인수 기준 |
|---|---|---|
| 네이티브 뷰포트 | Apple M5 Pro Metal, Rust 1.94.1, `wgpu 30.0.2`에서 실제 `opale::ViewportDesc`로 3×2 색 타깃을 만들었습니다. 한 인코더의 첫 패스에서 빨강으로 채운 뒤 두 번째 패스에서 그 뷰를 샘플링해 별도 타깃에 그렸습니다. 한 번 제출한 결과 6픽셀이 모두 RGBA `[255,0,0,255]`였습니다. 현행 Serge의 GPU/CPU 대칭·폴백 테스트 3건도 통과했습니다. | 외부 슬롯·세대 변경·클립·반투명 장면의 CPU/GPU 결과 대조, 변경 중인 이전 프레임의 안전한 해제 |
| 웹 뷰포트 | Chrome 158의 WebGPU에서 현재 텍스처의 픽셀을 읽었습니다. 63×31로 변경한 뒤 재확인하고, DOM 버튼과 노출된 캔버스의 좌표별 입력 대상을 검사했습니다. `device.destroy()`로 장치 상실을 발생시킨 뒤 새 캔버스의 2D 픽셀도 읽었습니다. | 제품 뷰포트의 실제 입력 전달, 두 슬롯의 독립 수명, WebGPU 초기화 실패와 언마운트 |
| 번들·SSR | JSON Schema 자체와 문서 예제를 검증했습니다(유효 2개, 잘못된 버전·모드·필드·해시·길이 등 거부 14개). 명세의 SSR 시그니처를 그대로 Rust 1.94.1에서 `Arc<dyn SergeSsrHandler>`로 컴파일했습니다. 첫 청크를 읽은 뒤 실제 Hyve 요청 스코프를 해제하자 다음 청크가 중단됐고 리다이렉트 변형도 반환됐습니다. | 실제 빌드 산출물로 정적·SSR 서빙, 누락 파일·가드 거부·304·HEAD·없는 경로·빌드 불일치 시 이전 번들 유지, 클라이언트 연결 해제 |
| 웹뷰 | 실제 `hyve-serde`로 14바이트 헤더 뒤의 `2^53+1` 순번을 왕복하고, 잘린 아카이브·크기 초과·버전/지문 불일치를 거부했습니다. 독립 순서 모델에서 전송·완료·ACK·리로드·옛 세대 전달·다음 입력의 길이 5 조합 7,776개를 실행한 뒤 전달을 재개하자 두 이벤트가 각각 한 번씩 처리됐습니다. | 실제 OS 웹뷰에서 저장 이벤트의 결과가 유실된 뒤 리로드, 이전 세대 무시, 두 번째 제출 제어, 주체 변경과 외부 탐색에서 브리지 폐기 |

웹뷰의 이벤트 순서 모델은 Serge 루트에서 `python3 docs/contracts/webview-ordering.py`로 재현합니다. 프로덕션 어댑터와 OS 웹뷰의 통합 실행은 표의 인수 단계에서 확인합니다. [실행 가능한 순서 모델](contracts/webview-ordering.py) · [뷰포트](03-adapters-and-pipeline.md#viewport-v1) · [번들·SSR](03-adapters-and-pipeline.md#bundle-v1) · [웹뷰](03-adapters-and-pipeline.md#webview-v1)

### 프리릴리스 기록

- **`0.1.0-alpha.0` (2026-11-26)**: 태그 `v0.1.0-alpha.0`은 `0ed448a`입니다. `hyve`·`orbic` `0.1.0-alpha.0`이 배포된 뒤 패치 없이 레지스트리 기준으로 `cargo publish --workspace --dry-run`을 리허설했고, 공개 크레이트 15개를 의존 순서대로 crates.io에 배포했습니다. **확정의 실행 방식을 바꿈**: 사용자 결정으로 §3 기준을 모두 채우기 전에 배포했습니다. **포크 검증**은 미완입니다. 아래 포크 기록의 대상은 소스를 편입하지 않은 고정 의존이라 `hyperish/docs` `05-development-rules.md` §7의 8번에 따라 충족으로 보지 않습니다. PR을 거치지 않았기 때문에 `main`이 아닌 `feat/phase1`의 커밋에 태그했습니다. 레지스트리에서 받아 확인한 것은 둘입니다. `hyve-cli`·`orbic-cli`·`serge-cli` `0.1.0-alpha.0`을 `cargo install --locked`로 받아 그것만 `PATH`에 두고 이 레포 플레이그라운드에서 `hyve preview --web`을 돌리면, 페이지와 `orbic` 백엔드 API가 `200`을 주고 Ctrl-C 뒤 남는 프로세스가 없습니다. 또 경로 없이 `hyve`·`serge` `=0.1.0-alpha.0`에만 의존하는 새 크레이트가 데스크톱용으로 빌드되고, 받은 `hyve-serge`로 웹 번들(`index.html`·`app.js`·`app_bg.wasm`)이 만들어집니다. `alpha.0`에서 알려진 것: 기본 `index.html`이 `/favicon.ico`를 요청해 콘솔에 404가 남고(`0f1d7a1`), `hyve-serge --version`이 단계 이름으로 해석됩니다(`c3115ea`). 둘 다 `alpha.1`에서 고쳤습니다.
- 이후 변경은 `0.1.0-alpha.1`로 올렸습니다(`9cf940c`).

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

2026-11-26 release 빌드에서 128행의 Flexbox 화면을 21회 교대로 비교했습니다. 513개 노드 레이아웃의 중앙값은 169 µs, 공개 어댑터의 TaffyTree는 205 µs였습니다. 루트·측정 여부·크기를 바꾼 36개 조합에서 결과 상자가 일치했습니다. 같은 텍스트의 캐시된 측정은 265 ns 대 공개 alpha.1의 1,280 ns였습니다. 511×251 CPU 프레임의 출력 중앙값은 23.9 µs로 기존 Pixmap 복사의 31.6 µs와 비교했고 반투명 경계의 RGBA를 대조했습니다. [레이아웃 비교](../crates/serge-native-layout/src/reference.rs) · [텍스트 비교](../scripts/bench-native-text.py) · [프레임 비교](../crates/serge-native-cpu/src/lib.rs)

웹에서는 tachys의 텍스트 갱신 경로를 편입해 서버가 만든 텍스트 노드를 하이드레이션한 뒤 같은 노드에서 값만 바꿉니다. 절반의 값이 바뀌는 release WASM을 Chrome에서 21회 측정한 갱신당 중앙값은 Serge 77.9 ns, 기존 원소의 `textContent` 교체 197.4 ns, tachys 원본 방식 77.4 ns였습니다. 빈 값과 동일 값의 연속 갱신도 노드를 보존했습니다. [텍스트 갱신과 브라우저 테스트](../crates/serge-web-dom/src/text/mod.rs)

Apple 타깃에서 `serge/native-metal`을 켜면 같은 WGSL을 Metal 셰이더로 컴파일하고 UI 인스턴스와 외부 뷰포트를 같은 Metal 명령 버퍼에 그립니다. M5 Pro에서 256개 사각형을 한 번에 그린 결과는 wgpu 경로와 모든 바이트가 일치했습니다. 같은 화면을 그려 다시 읽는 release 빌드의 21회 측정 중앙값은 Metal 직접 호출 232.6 µs, wgpu 250.5 µs였습니다. 별도로 1×·2× 화면에서 CPU와 픽셀을 대조하고 실제 AppKit 창 두 개에서 표시·독립 종료를 확인했습니다. [네이티브 배치와 비교 테스트](../crates/serge-native-gpu/src/metal.rs) · [외부 뷰포트 검사](../crates/serge-native-gpu/tests/viewports.rs)

vello_cpu 0.4.0의 타깃 초기화 경로를 편입해 전체를 덮는 첫 불투명 사각형을 배경색으로 넘깁니다. 단색 화면은 RGBA 버퍼를 바로 채웁니다. 배율·크기·외곽·반투명도를 바꾼 54개 조합을 원본과 바이트 단위로 대조했습니다. 641×361 단색 프레임 release 중앙값은 13.2 µs(11.1~13.7), 기존 Vello 사각형은 37.3 µs(37.0~38.8)였습니다. [타깃 초기화와 비교 테스트](../crates/serge-native-cpu/src/target/mod.rs)
