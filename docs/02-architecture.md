# 02. 아키텍처와 크레이트 구조 (serge)

> 상태: `apps/`·`crates/` 모노레포·`hyve` 코어 `ref`·웹(DOM)+네이티브(GPU/CPU) 통합 구조는 **확정**, 세부 크레이트 분할은 **제안** · 기준일: 2026-11-25

`serge` 레포는 Cargo 워크스페이스 기반의 모노레포(`apps/`, `crates/`)로 구성합니다(`hyperish/docs`의 `05-development-rules.md` §1). 하나의 공통 UI 코어 위에 **Web(DOM/SSR) 렌더 타깃**과 **Native(GPU/CPU) 렌더 타깃**이 대칭으로 올라갑니다.

## 1. 워크스페이스 디렉터리 구조 (제안)

```
serge/
  apps/
    serge-cli/                 # `hyve-serge` 실행 파일 (`hyve`가 호출하는 번들링·개발 서버·네이티브 실행 단계)
    serge-playground/          # 웹(DOM) 및 네이티브(GPU/CPU) 통합 컴포넌트·120fps 쇼케이스 앱
  crates/
    serge/                     # 사용자 진입 크레이트 (단일 의존성 재수출 + prelude)
    serge-core/                # 뷰 트리, 폼(`hyve-validate`), Suspense/ErrorBoundary/낙관적 UI, 테마·i18n, PaintCtx/ScenePrimitive
    serge-macros/              # 공통 UI 문법(QML/Slint + Dioxus rsx! 스타일 `ui!`) proc macro (hyve-parser 참조)
    serge-style/               # 웹·네이티브 공용 스타일·디자인 토큰·레이아웃 IR (웹 타깃에서는 CSS로 직결되어 WASM에 솔버 미포함)
    serge-router/              # 웹(History API) 및 네이티브(네비게이션 스택) 공용 타입 안전 라우터·중첩 레이아웃·가드

    # --- Web (DOM) 타깃 크레이트 ---
    serge-web-dom/             # 모든 DOM/웹 기능 빌더 + Leptos식 템플릿 복제·수술적 패칭 + `persistent_signal`·오프라인 큐·a11y
    serge-web-ssr/             # 스트리밍 HTML SSR·정적 사이트 생성(SSG)·`<Title>`/`<Meta>` Head 수집 (`orbic` 서빙 연동용)
    serge-web-ionic/           # Ionic 레퍼런스 기반 적응형 웹 UI 컴포넌트·제스처·트랜지션
    serge-web-dev/             # 최소 CRA 수준의 로컬 개발용 프리뷰·번들·HMR 데브 서버 (`hyve preview` 연동)

    # --- Native (GPU + 필수 CPU 폴백) 타깃 크레이트 ---
    serge-native/              # 네이티브 런타임: 유지 트리·시그널 동기화·장면 수집·히트 테스트·입력·UI 작업 실행기·헤드리스 하네스 (제안, 2026-11-25)
    serge-native-layout/       # 네이티브 전용 고속 Flexbox/Grid/절대배치 레이아웃 솔버 (`taffy` 포크 계열, Web WASM 번들과 완전 분리)
    serge-native-gpu/          # GPUI/Rune식 GPU 배칭 렌더 백엔드 (쿼드·경로·그림자·글리프·ExternalTexture 제로카피 합성)
    serge-native-cpu/          # 필수 CPU 소프트웨어 폴백 렌더 백엔드 (동일 ScenePrimitive 소비)
    serge-native-text/         # 네이티브 폰트 탐색, 텍스트 셰이핑, 줄바꿈, 서브픽셀/SDF 글리프 아틀라스
    serge-native-sys/          # 크로스플랫폼 HW(`serge::hw`: 센서·카메라 제로카피·생체인증·펜슬/햅틱·BLE) & SW(`serge::sys`: 네이티브 Alert/ActionSheet·파일·Keychain·알림·딥링크·AccessKit)
    serge-platform-desktop/    # macOS/Windows/Linux 멀티 윈도우·메뉴바/MenuBarExtra·트레이·단축키·로컬 Shm IPC·VSync 어댑터
    serge-platform-ios/        # iOS UIKit/Metal·Safe Area·키보드 회피 어댑터 + Dynamic Island/WidgetKit(`#[ios::live_activity]`) SwiftUI 코드젠 브리지·App Intents
    serge-platform-android/    # Android NativeWindow·Vulkan/GLES·Safe Area·키보드 회피·폴더블 Posture 어댑터 + Ongoing Notification/위젯 Glance 코드젠 브리지

    # --- 공용 통신(3-Tier: In-Process / Shm IPC / Remote Dual-Rail) 및 옵트인 웹뷰 어댑터 ---
    serge-adapter-client/      # 인프로세스(`SharedRingBuf`), 로컬 IPC(OS `shm` + `hyve-serde` RPC), 원격 서버(Dual-Rail) 통합 클라이언트
    serge-adapter-webview/     # [옵트인 전용] 네이티브 창 일부에 `serge-web-dom` 출력물을 웹뷰 서피스로 띄울 때만 사용
```

## 2. 계층 의존 방향과 `hyve` 코어(`ref`) 수직화

```
  [ Web 타깃: serge-web-{dom,ssr,ionic,dev} ]    [ Native 타깃: serge-native-{layout,gpu,cpu,text,sys} + serge-platform-* ]
                       \                                              /
                        ▼                                            ▼
                   [ serge-core / serge-style / serge-router / serge-macros ]
                                             │
                                             ▼ (코어 ref)
                       [ hyve (core / state / parser / serde / api) ]
```

- **단일 반응성 및 UI 문법 (`hyve-state`, `hyve-parser`)**: 웹 타깃과 네이티브 타깃이 모두 `hyve-state`와 `serge-core` 뷰 트리를 공유하므로, 컴포넌트와 비즈니스 상태 로직을 두 번 작성하지 않습니다.
- **네이티브 GPU & CPU 백엔드의 대칭성**: 네이티브 타깃에서는 `serge-core`가 플랫폼 중립 `ScenePrimitive` 리스트를 생성하고, `serge-native-gpu`와 `serge-native-cpu`(필수 폴백)가 동일한 프리미티브를 소비해 렌더링합니다.
- **네이티브 WebView 격리 보장**: `serge-native-*`와 `serge-platform-*` 기본 트리에는 어떠한 WebView 의존성도 링크되지 않습니다. 웹뷰가 꼭 필요한 응용에서만 같은 레포 내의 옵트인 어댑터(`serge-adapter-webview`)를 켜서 `serge-web-dom` 출력물을 웹뷰 서피스로 마운트합니다.

## 3. 확정 MIT 포크 스택 및 내부 코어 `unit optimization` 타깃 (2026-11-19 확정)

표의 버전·라이선스는 crates.io 레지스트리(`cargo info <크레이트>@<버전>`)에서 2026-11-24에 확인한 스냅샷입니다. 확정된 것은 베이스 크레이트의 선택이며, 포크 기준 리비전은 착수 시점에 다시 확인해 고정합니다(`hyperish/docs` `05-development-rules.md` §7).

| 모듈 | 베이스 크레이트 (확인 버전 · 라이선스) | 포크 후 내부 코어 개조(1·2단계 `unit optimization`) 방향 |
|---|---|---|
| `serge-web-dom` | `tachys` (`0.3.0-rc1`, `MIT`) | 극단적 제네릭 튜플 중첩(컴파일 시간·WASM 팽창)을 걷어내고 `serge-macros`(`ui!`)가 정적 `<template>` + 고정 슬롯 오프셋 패칭 코드만 직접 방출하도록 경량화 |
| `serge-native-layout` | `taffy` (`0.15.1`, `MIT`) | Web WASM 번들과 완전 분리; `serge-style`의 **컴팩트 SoA 스타일 테이블 + 더티 서브트리 증분 계산**으로 내부 메모리 레이아웃 개조 |
| `serge-native-gpu` | `wgpu` (`30.0.2`, `MIT OR Apache-2.0`) + `vello` (`0.12.0`, `Apache-2.0 OR MIT`) *(GPUI는 아키텍처 레퍼런스 전용, 포크 금지)* | Phase 1은 단일 거대 인스턴스 버퍼 배칭(프레임당 1~3 드로우콜)으로 `wgpu` 제출 오버헤드 극소화 → Phase 2~3은 `ScenePrimitive` 아래를 **`wgpu-hal` 직결 또는 네이티브 Metal/Vulkan 직접 백엔드**로 교체 |
| `serge-native-cpu` | `vello_cpu` (`0.4.0`, `Apache-2.0 OR MIT`) | `vello`와 동일 씬 모델을 공유하는 SIMD 소프트웨어 래스터라이저로 필수 CPU 폴백 대칭 구현 |
| `serge-native-text` | `parley` (`0.13.0`, `Apache-2.0 OR MIT`) + `swash` (`0.2.10`, `Apache-2.0 OR MIT`) | Linebender 텍스트 셰이핑·글리프 래스터라이저를 `serge-native-gpu`/`cpu` 아틀라스에 직결 |
| `serge-platform-*` · `serge-native-sys` | `winit` (`0.31.0-beta.4`, `Apache-2.0` — **포크 없는 외부 의존성 전용**), `accesskit` (`0.25.2`, `MIT OR Apache-2.0`) | `winit`은 외부 의존성으로만 사용해 순수 MIT 유지; 추후 OS 창 제어 최적화 필요 시 `raw-window-handle`(`0.6.2`, `MIT OR Apache-2.0 OR Zlib`) 기반 자체 MIT OS 어댑터로 대체 |

### 3.1 Phase 1 착수 방식 (2026-11-25)

포크 표의 베이스 크레이트 선택은 그대로이고, Phase 1 프로토타입은 포크를 만들지 않고 베이스 크레이트를 정확한 버전(`=`)으로 고정해 의존합니다. 포크본과 원본의 차이·측정 기록(`05-implementation-plan.md` §3 포크 검증)은 **미완**입니다.

- `serge-native-layout`: `taffy` `=0.15.1` (확인 시점의 최신 `0.16.0`은 공개 하루째라 보류).
- `serge-native-cpu`: `vello_cpu` `=0.4.0`. 글리프도 `vello_cpu`가 `parley`의 폰트 데이터로 그리므로 `swash`는 쓰지 않습니다(확정의 실행 방식을 바꿈).
- `serge-native-text`: `parley` `=0.13.0`.
- `serge-web-dom`: `tachys`는 레퍼런스로만 보고 직접 구현합니다. 포크 방향이 내부를 다시 쓰는 것이고 `tachys`는 `reactive_graph`에 묶여 있어 `hyve-state`와 맞지 않습니다(확정의 실행 방식을 바꿈).
- `serge-platform-desktop`: `winit` `=0.30.13`(표의 `0.31.0-beta.4` 대신 안정판), CPU 프레임 표시에 `softbuffer` `=0.4.8`(새 외부 의존성, `MIT OR Apache-2.0`). GPU 경로(`wgpu` `30.0.2`)는 **미완**이라 자동 선택은 CPU이고, `HYPERISH_RENDER_BACKEND=gpu`는 오류입니다.
- 매크로: `syn` `2.0.119`(표의 `3.0.10` 대신. 생태계의 `quote`·`proc-macro2`와 맞춤).
- 원격 전송(네이티브): `hyper` `=1.12.0`, `hyper-util` `=0.1.21`, `http-body-util` `=0.1.5`, `http` `=1.5.0`, `bytes` `=1.12.2`. 개발 도구: `serde_json` `=1.0.154`. 모두 `MIT` 또는 `MIT OR Apache-2.0`.
- 최소 Rust 버전은 워크스페이스 전체 `1.94`입니다(쇼케이스가 링크하는 `orbic`의 요구).

외부 의존성(포크하지 않음, 2026-11-24 `cargo info` 확인): 웹 바인딩 `wasm-bindgen` (`0.2.130`), `wasm-bindgen-futures` (`0.4.80`), `web-sys` (`0.3.107`), `js-sys` (`0.3.107`), 매크로용 `syn` (`3.0.10`), `quote` (`1.0.47`), `proc-macro2` (`1.0.107`) — 모두 `MIT OR Apache-2.0`. 네이티브의 비동기 I/O는 패밀리 공통 런타임 `tokio` (`1.54.1`, `MIT`)를 쓰고, 웹은 브라우저 이벤트 루프를 씁니다(`hyve/docs/02-architecture.md` §2.5). 이 목록에 없는 의존성은 `hyperish/docs` `05-development-rules.md` §7의 외부 의존성 규칙에 따라 추가 기록합니다.
