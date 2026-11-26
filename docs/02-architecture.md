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
    serge-web-dev/             # 최소 CRA 수준의 로컬 개발 서버·번들·HMR (`hyve dev` 연동)

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

포크 표의 베이스 크레이트 선택은 그대로이고, Phase 1 프로토타입은 포크를 만들지 않고 베이스 크레이트를 정확한 버전(`=`)으로 고정해 의존합니다. 달라진 점은 `05-implementation-plan.md`의 포크 기록에 있고, 소스 편입과 원본 대비 측정(§3 포크 검증)은 **미완**입니다.

- `serge-native-layout`: `taffy` `=0.15.1` (확인 시점의 최신 `0.16.0`은 공개 하루째라 보류).
- `serge-native-cpu`: `vello_cpu` `=0.4.0`. 글리프도 `vello_cpu`가 `parley`의 폰트 데이터로 그리므로 `swash`는 쓰지 않습니다(확정의 실행 방식을 바꿈).
- `serge-native-text`: `parley` `=0.13.0`. Linux에서는 `fontique` `=0.13.0`의 `fontconfig-dlopen`을 켜 fontconfig를 실행 시점에 불러오므로 빌드에 개발 패키지·`pkg-config`가 필요 없습니다.
- `serge-web-dom`: `tachys`는 레퍼런스로만 보고 직접 구현합니다. 포크 방향이 내부를 다시 쓰는 것이고 `tachys`는 `reactive_graph`에 묶여 있어 `hyve-state`와 맞지 않습니다(확정의 실행 방식을 바꿈).
- `serge-platform-desktop`: `winit` `=0.30.13`(표의 `0.31.0-beta.4` 대신 안정판), CPU 프레임 표시에 `softbuffer` `=0.4.8`(새 외부 의존성, `MIT OR Apache-2.0`). 창을 만든 뒤 폴백 정책대로 GPU 표면·디바이스를 만들어 보고, 실패하면 CPU로 그립니다.
- `serge-native-gpu`: `wgpu` `=30.0.2`(`metal`·`vulkan`·`dx12`·`wgsl`만, 기본 기능 끔). 표의 `vello` `0.12.0`은 Phase 2 이후라 아직 쓰지 않습니다. 글리프와 경로의 커버리지 마스크는 `swash`가 아니라 CPU 경로와 같은 `vello_cpu`가 만듭니다(`serge-native-cpu`의 `MaskRasterizer`). 같은 래스터라이저라 두 경로의 글자가 같게 나오고 새 의존성이 없습니다. 구조와 허용 오차는 `03-adapters-and-pipeline.md` §1.2에 있습니다.
- 매크로: `syn` `2.0.119`(표의 `3.0.10` 대신. 생태계의 `quote`·`proc-macro2`와 맞춤).
- 원격 전송(네이티브): `hyper` `=1.12.0`, `hyper-util` `=0.1.21`, `http-body-util` `=0.1.5`, `http` `=1.5.0`, `bytes` `=1.12.2`. 개발 도구: `serde_json` `=1.0.154`. 모두 `MIT` 또는 `MIT OR Apache-2.0`.
- 원격 전송의 TLS(네이티브, 2026-11-25, 라이선스는 `cargo metadata`로 확인): `hyper-rustls` `=0.27.10`, `rustls` `=0.23.46`, `tokio-rustls` `0.26.6`(이상 `Apache-2.0 OR ISC OR MIT` 또는 `MIT OR Apache-2.0`), `rustls-platform-verifier` `=0.7.1`(`MIT OR Apache-2.0`). 이들이 끌어오는 것: `ring` `0.17.14`(`Apache-2.0 AND ISC`), `rustls-webpki` `0.103.17`·`untrusted` `0.9.0`(`ISC`), `rustls-pki-types` `1.16.0`·`zeroize` `1.9.2`·`getrandom` `0.2.17`(`MIT OR Apache-2.0`), `subtle` `2.6.1`(`BSD-3-Clause`), macOS·iOS의 `security-framework` `3.7.0`·`security-framework-sys` `2.17.0`·`core-foundation` `0.10.1`(`MIT OR Apache-2.0`), Linux의 `rustls-native-certs` `0.8.5`(`Apache-2.0 OR ISC OR MIT`)·`openssl-probe` `0.2.2`(`MIT OR Apache-2.0`), Windows의 `schannel` `0.1.30`(`MIT`), Android의 `rustls-platform-verifier-android` `0.2.0`(`MIT OR Apache-2.0`), `webpki-root-certs` `1.0.12`(`CDLA-Permissive-2.0`, 인증서 데이터). 모두 포크하지 않는 외부 의존성이고 `MIT` 코드와 함께 배포할 수 있는 허용 라이선스입니다. 암호 구현은 `aws-lc-rs` 대신 `ring`입니다(빌드에 CMake가 필요 없고 교차 컴파일이 쉬움). 웹 빌드에는 들어가지 않습니다(`crates/serge/tests/deps.rs`). 테스트 전용: `rcgen` `=0.14.10`(`MIT OR Apache-2.0`)과 그 의존성.
- 최소 Rust 버전은 워크스페이스 전체 `1.94`입니다(쇼케이스가 링크하는 `orbic`의 요구).

외부 의존성(포크하지 않음, 2026-11-24 `cargo info` 확인): 웹 바인딩 `wasm-bindgen` (`0.2.130`), `wasm-bindgen-futures` (`0.4.80`), `web-sys` (`0.3.107`), `js-sys` (`0.3.107`), 매크로용 `syn` (`3.0.10`), `quote` (`1.0.47`), `proc-macro2` (`1.0.107`) — 모두 `MIT OR Apache-2.0`. 네이티브의 비동기 I/O는 패밀리 공통 런타임 `tokio` (`1.54.1`, `MIT`)를 쓰고, 웹은 브라우저 이벤트 루프를 씁니다(`hyve/docs/02-architecture.md` §2.5). 이 목록에 없는 의존성은 `hyperish/docs` `05-development-rules.md` §7의 외부 의존성 규칙에 따라 추가 기록합니다.

### 선택형 네이티브 어댑터 (2026-11-26)

`serge/native-system`은 데스크톱 클립보드에 arboard 3.7.0, 파일·메시지 패널에 rfd 0.18.0, 사용자별 앱 데이터 경로에 directories 6.0.0을 사용합니다. `desktop-system`은 global-hotkey 0.10.0으로 OS 전역 단축키를 등록하며 창의 이벤트 루프에서 명령을 받습니다. rfd는 MIT, 나머지는 MIT 또는 Apache-2.0입니다. Linux X11 인수에서 실제 GTK 패널, xclip, OS 키 이벤트를 사용합니다. [시스템 어댑터](../crates/serge-native-sys/src/lib.rs) · [X11 인수](../apps/serge-playground/tests/system_x11.rs)

`serge/webview`는 Wry 0.58.0(MIT 또는 Apache-2.0)을 선택적으로 연결합니다. macOS WKWebView와 Linux X11 WebKitGTK의 로컬 자식 문서에서 양방향 레일과 문서 재로딩을 검증했습니다. [웹뷰 동기화 계약](03-adapters-and-pipeline.md#webview-v1) · [인수 테스트](../apps/serge-playground/tests/webview.rs)


기기의 보안 저장소는 keyring 3.6.3(MIT 또는 Apache-2.0)으로 Apple Keychain·Windows Credential Manager·Linux Secret Service를 사용합니다. Android는 앱 전용 Keystore의 AES-GCM 키와 원자적 파일 교체를 사용합니다. Android NDK 0.9.0과 JNI 0.22.4로 센서 큐·AAudio·사용자 대화상자에 접근합니다. Apple에서는 objc2 0.6.5, objc2-core-motion 및 objc2-local-authentication 0.3.2로 Core Motion과 LAContext를 호출합니다. [기기 SDK 소스](../crates/serge-native-sys/src/) · [의존성](../crates/serge-native-sys/Cargo.toml)

기기 어댑터의 선택 기능은 `serge/bluetooth`의 btleplug 0.13.5(BSD-3-Clause), `serge/midi`의 midir 0.11.2(MIT), Apple Metal과 Android Vulkan용 `serge/native-camera`입니다. Android는 ash 0.38(MIT 또는 Apache-2.0)과 NDK 미디어 API로 가져온 카메라 버퍼를 샘플링합니다. Android BLE DEX를 만드는 스크립트가 원본 리비전과 체크섬을 확인합니다. Android MIDI는 API 29 이상을 사용합니다. Apple 마이크는 objc2-av-foundation 및 objc2-avf-audio 0.3.2(MIT 선택 가능)로 권한, AVAudioEngine, 샘플 링을 연결합니다. 폴더블 기능을 선택한 Android 앱은 AndroidX WindowManager 1.3.0과 Core 1.8.0(Apache-2.0)을 사용합니다. [BLE 소스와 고지](../crates/serge-native-sys/android/ble/) · [기기 SDK](../crates/serge-native-sys/src/) · [카메라 텍스처](../crates/serge-native-gpu/src/camera.rs)

2026-11-26에 Taffy 0.15.1의 트리 뷰를 Serge의 배열 인덱스에 맞춰 편입했습니다. Flexbox 계산은 원본을 쓰며 페인트와 히트 테스트가 반올림 결과 배열을 공유합니다. Parley 0.13.0의 글리프 위치 계산은 씬 글리프를 직접 생성하고, 측정·페인트가 캐시된 문단을 빌립니다. CPU 프레임과 마스크는 Vello의 `PixmapMut`에 RGBA 버퍼를 바로 전달합니다. [Taffy 소스와 MIT 고지](../crates/serge-native-layout/src/compact/) · [Parley 소스와 MIT 고지](../crates/serge-native-text/src/glyph_run/) · [CPU 출력](../crates/serge-native-cpu/src/lib.rs)

모바일 서명에는 P-256과 SHA-256을 씁니다. AndroidKeyStore의 명명된 개인 키는 내보낼 수 없고 장치별 하드웨어 보호 여부를 확인할 수 있습니다. Apple은 Security framework 3.7.0의 SecKey와 Secure Enclave로 일시 키를 생성합니다. 기본 보호는 장치가 잠금 해제된 동안이며 백그라운드에서 서명할 앱은 최초 잠금 해제 이후 접근을 명시할 수 있습니다. [Android 키](../crates/serge-native-sys/src/android/signing.rs) · [Apple 키](../crates/serge-native-sys/src/apple.rs) · [Apple 접근 정책](https://developer.apple.com/documentation/security/protecting-keys-with-the-secure-enclave)
