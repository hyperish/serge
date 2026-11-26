# 03. 파이프라인과 호환 어댑터 (serge)

> 타깃별 파이프라인 통합과 아래 v1 호환 계약은 **확정**입니다. 기존 단계별 제안과 구현 기록은 해당 절에 표시합니다. 기준일 2026-11-26

`serge`는 하나의 공통 반응형 뷰 트리(`serge-core` + `hyve-state`)로부터 **Web(DOM) 렌더링 파이프라인**과 **Native(GPU/CPU) 렌더링 파이프라인** 두 갈래의 실행 경로를 제공합니다.

## 1. 타깃별 렌더링 파이프라인

### 1.1 Web 타깃 파이프라인 (Leptos 레퍼런스)
1. **컴파일 타임 정적 분리 (`serge-macros`)**: 정적 HTML 골격 문자열과 동적 바인딩 지점(시그널, 이벤트, 슬롯)을 분리합니다.
   - **구현 (제안, 2026-11-25)**: 매크로는 HTML 문자열 대신 위젯 노드마다 그 자리를 뜻하는 `static`(`Site`, 같은 `ui!` 안에서 감싼 위젯의 자리를 부모로 가짐)과, 값마다 소스 리터럴인지(`Prop::Const`)를 방출합니다. 골격 HTML은 스타일이 런타임 테마로 정해지므로 웹 타깃이 그 자리의 첫 마운트 때 만들어 캐시합니다.
2. **클라이언트 DOM 마운트 및 수술적 패칭 (`serge-web-dom`)**: 최초 마운트 시 `<template>` 노드를 `cloneNode(true)`로 복제하고, 시그널 변경 시 가상 DOM 전체 비교 없이 해당 DOM 텍스트·속성만 즉시 갱신합니다.
   - **Phase 1 프로토타입 (2026-11-25)**: 동적 프로퍼티마다 효과 하나로 그 텍스트·속성만 갱신하고, `if` 영역과 키 있는 `for` 목록은 주석 마커 사이에서 갱신합니다(값이 같은 항목은 DOM 노드를 옮김). 스타일은 `serge-style`이 낸 인라인 선언과 테마 CSS 변수입니다.
   - **템플릿 복제 (제안, 2026-11-25)**: `ui!`에 적힌 위젯은 자리마다 템플릿 하나를 둡니다. 템플릿은 그 자리에서 처음 만든 위젯으로 만든 분리된 원본 노드로, 소스 리터럴인 스타일·속성·`web::` 속성을 굽고, 같은 `ui!` 안에 바로 적힌 자식 위젯을 중첩해 담고, 그 밖의 자식(`if`·`for`·`{expr}`·컴포넌트) 자리에는 빈 주석을 둡니다. 이후 그 자리의 위젯은 원본을 `cloneNode(true)`로 복제한 뒤 구워지지 않은 프로퍼티·핸들러·자식만 채웁니다. 입력의 값과 체크 상태처럼 HTML 속성이 아니라 살아 있는 프로퍼티인 값은 굽지 않습니다. 코드로 만든 위젯(자리 없음)은 DOM 호출로 만듭니다. `for` 목록 항목은 항목 수와 상관없이 원소 생성 없이 복제됩니다(100행 목록: 원소 생성 6번, 복제 101번, `crates/serge/tests/web.rs`).
3. **스트리밍 SSR & Hydration (`serge-web-ssr`)**: `orbic` 백엔드와 결합 시 비동기 리소스가 준비되는 대로 HTML 청크와 직렬화된 상태(`hyve-serde`)를 스트리밍하고 클라이언트에서 이벤트·시그널만 복원합니다.

### 1.2 Native 타깃 파이프라인 (GPUI & `anomalyco/rune` 레퍼런스)
1. **반응형 더티 마킹 (`hyve-state` 연동)**: 터치·마우스·키보드 입력이나 상태 변경 시 해당 시그널을 구독하는 네이티브 뷰 서브트리만 더티(dirty)로 표시됩니다.
2. **레이아웃 & 텍스트 셰이핑 (`serge-native-layout`, `serge-native-text`)**: 변경된 서브트리의 레이아웃 제약을 계산하고 글리프 런을 셰이핑·캐싱합니다.
3. **`ScenePrimitive` 수집 (`serge-core`)**: 배경, 테두리, 그림자, 벡터 패스, 글리프 런, 외부 3D/GPU 텍스처 서피스를 클립 계층에 맞춰 정렬된 `ScenePrimitive` 배치로 뽑아냅니다.
4. **GPU 실행 또는 CPU 폴백 (`serge-native-gpu` / `serge-native-cpu`)**:
   - **기본 경로 (`serge-native-gpu`)**: 인스턴스 버퍼와 아틀라스를 통해 최소 드로우콜로 GPU에서 120fps 합성.
   - **필수 폴백 경로 (`serge-native-cpu`)**: GPU 초기화 실패, 가상 디스플레이, 헤드리스 CI 테스트 환경에서 SIMD 최적화된 CPU 래스터라이저가 동일한 `ScenePrimitive`를 픽셀 버퍼에 렌더링.
   - **Phase 1 프로토타입 (2026-11-25)**: 유지 트리와 입력은 `serge-native`, 창은 `winit`, CPU 프레임 표시는 `softbuffer`입니다. 정책 함수(`choose_backend`)는 환경 변수 값·헤드리스 여부·디바이스 생성 결과만으로 정해지는 순수 함수로 테스트합니다.
   - **GPU 경로 (제안, 2026-11-25)**: `serge-native-gpu`가 `wgpu`로 한 프레임을 **인스턴스 버퍼 하나·파이프라인 하나·드로우콜 하나**로 그립니다. 인스턴스마다 종류(채움·테두리·글리프·컬러 글리프·경로), 사각형, 색, 클립 사각형을 싣고, 클립은 셰이더에서 커버리지로 곱하므로 클립이 바뀌어도 드로우콜이 나뉘지 않습니다. 사각형과 테두리는 셰이더의 거리 함수로 안티에일리어싱합니다. 글리프와 경로(`Line`·`Polygon`)는 CPU 경로와 같은 `vello_cpu`로 커버리지 마스크를 만들어 아틀라스에 올립니다(글리프 아틀라스는 글꼴·글리프·크기·배율·1/64 px로 양자화한 소수 위치를 키로 한 캐시, 경로 아틀라스는 프레임마다). 같은 래스터라이저·변환을 정수 픽셀만큼 옮겨 쓰므로 글리프와 경로는 CPU 결과와 구조적으로 같습니다. 경로의 GPU 래스터화는 Phase 2입니다. 색 공간은 CPU 경로와 같이 sRGB 인코딩 값에 사전 곱셈 알파로 섞습니다(sRGB가 아닌 렌더 타깃).
   - **렌더러 대칭의 허용 오차 (제안, 2026-11-25, 비교 전에 고정)**: 같은 `ScenePrimitive`를 같은 크기·배율로 그린 두 프레임에서 픽셀마다 RGBA 채널 차의 최댓값 `d`(0~255)를 잽니다. **`d`의 평균이 0.5 이하**이고 **`d > 32`인 픽셀이 전체의 0.1% 이하**면 일치로 봅니다. 차이가 남는 곳은 둥근 모서리(거리 함수 대 해석적 면적)와 반투명 합성의 반올림, 클립 경계뿐이라는 가정에서 정한 값이며, 넘으면 구현을 고칩니다.
   - **전환 정책 (확정, 2026-11-24)**: GPU 디바이스를 만들지 못하면 자동으로 CPU 경로로 전환합니다. 환경 변수 `HYPERISH_RENDER_BACKEND`에 `cpu` 또는 `gpu`를 명시할 수 있고, `gpu`를 명시했는데 실패하면 폴백하지 않고 오류를 냅니다. 헤드리스 테스트는 CPU 경로를 씁니다. 명시한 환경 변수가 항상 우선하고, 변수가 없을 때만 헤드리스 테스트의 CPU 기본값과 자동 전환이 적용됩니다. 가상 디스플레이는 따로 감지하지 않고 디바이스 생성 실패로 다룹니다. 외부 뷰포트를 연결한 뒤 실행 중 디바이스를 잃는 경우는 [뷰포트 v1](#viewport-v1)의 전환 규칙을 따릅니다.

<a id="viewport-v1"></a>

### 1.3 외부 뷰포트 v1 (확정, 2026-11-26)

| 경계 | 생산자와 소비자의 계약 |
|---|---|
| 네이티브 GPU | Serge가 `Device`, `Queue`, 색 타깃과 프레임 인코더를 소유합니다. 생산자(Opale 등)는 준비 시 같은 디바이스에서 리소스를 만들고, 그릴 때 호스트가 빌려 준 `&TextureView`와 `&mut CommandEncoder`에 패스를 기록합니다. 생산 패스를 끝낸 다음 호스트가 같은 텍스처를 샘플링해 해당 장면 순서·클립·불투명도로 합성하고 한 큐에 제출합니다. 프레임마다 CPU로 읽거나 디바이스 전체를 기다릴 필요가 없습니다. |
| 장면·수명 | `ScenePrimitive::ExternalTexture`에는 슬롯 ID·세대·논리 좌표의 목적 사각형만 담습니다. GPU 뷰와 버퍼는 렌더러 레지스트리가 보관합니다. 크기 변경·언마운트·디바이스 상실 시 세대를 올립니다. 과거 세대는 그리지 않고 새 프레임을 요청합니다. 제출한 옛 리소스는 해당 제출 완료까지 유지합니다. 이미 상실한 디바이스는 완료 신호를 기다리지 않고 해당 세대 전체를 폐기합니다. 이 핸들은 로컬 전용이며 IPC·공유 링에 직렬화하지 않습니다. |
| 픽셀 | 논리 크기 × 디바이스 배율을 가장 가까운 물리 픽셀로 반올림합니다. 한 축이라도 0이면 할당·그리기를 쉽니다. 네이티브 출력은 2D, 한 레이어·한 밉·단일 샘플의 `Rgba8Unorm`, `RENDER_ATTACHMENT \| TEXTURE_BINDING`입니다. 좌상단 원점, sRGB로 인코딩한 RGB에 알파를 사전 곱한 값, 선형 알파를 사용합니다. 선형 장면이나 MSAA를 쓰는 생산자는 전달 전에 변환·리졸브합니다. 기존 CPU·GPU UI의 블렌딩 규칙과 같습니다. |
| CPU | 같은 슬롯·크기·세대로 패딩 없는 행 우선 RGBA8 픽셀(행 길이 `width × 4`)을 받습니다. 호스트가 크기와 버퍼 길이를 확인합니다. CPU 프레임을 제공할 수 없는 생산자는 앱이 지정한 대체 뷰를 표시합니다. 기본 대체 뷰는 해당 슬롯의 배경과 접근 가능한 “뷰포트 사용 불가” 상태입니다. 나머지 UI와 입력은 계속 동작합니다. |
| 웹 | Serge가 전용 `<canvas>`와 레이아웃·포커스를 소유합니다. 생산자는 이 캔버스의 WebGPU 컨텍스트를 선호 포맷, sRGB, `premultiplied`로 구성하고 프레임마다 현재 텍스처를 얻습니다. CSS 크기와 물리 backing 크기를 함께 갱신합니다. DOM 오버레이가 먼저 입력을 받고, 캔버스 입력은 슬롯의 로컬 물리 좌표로 전달합니다. WebGPU가 없거나 디바이스를 잃으면 세대를 폐기하고 새 캔버스의 2D 컨텍스트에 CPU 프레임을 표시합니다(2D 업로드 시 straight RGBA로 변환). 언마운트는 프레임 콜백·관찰자·이벤트 리스너와 컨텍스트를 해제합니다. |

v1은 네이티브에서 단일 디바이스를 공유하고 웹에서는 DOM 합성을 사용합니다. IOSurface·DXGI·dma-buf·AHardwareBuffer로 다른 프로세스/디바이스의 메모리를 가져오는 경로는 플랫폼별 동기화와 소유권 계약이 추가로 필요해 v1에서 제외합니다. 네이티브 장치 상실 시 기본 모드는 CPU로 전환하고, 명시한 `gpu` 모드는 오류를 보고합니다. `serge::viewport::Viewport`는 마운트마다 독립 슬롯을 만들고 `ScenePrimitive::ExternalTexture`로 합성합니다. CPU 생산자와 네이티브 `GpuViewport` 또는 브라우저 `WebViewport::webgpu`를 등록할 수 있습니다. [현재 장면 타입](../crates/serge-core/src/paint.rs) · [wgpu 큐](https://docs.rs/wgpu/30.0.2/wgpu/struct.Queue.html#method.submit) · [WebGPU 리소스와 디바이스](https://www.w3.org/TR/webgpu/#abstract-opdef-valid-to-use-with) · [캔버스 컨텍스트 수명](https://www.w3.org/TR/webgpu/#canvas-rendering)

Metal 인수는 두 생산자 패스를 같은 인코더에 기록하고 클립, 반투명 겹침, 위에 놓인 일반 UI, 리사이즈·언마운트 뒤 과거 키를 CPU 픽셀과 대조합니다. Chrome 인수는 실제 WebGPU 텍스처에서 색을 읽고 기기 상실과 CSS 크기 변경 후 2D 폴백의 픽셀과 backing 크기를 확인합니다. [네이티브 합성 테스트](../crates/serge-native-gpu/tests/viewports.rs) · [브라우저 테스트](../crates/serge/tests/web.rs)


**Tier 2 멀티프로세스.** [패밀리 코어 계약](https://github.com/hyperish/docs/blob/main/docs/02-core-contracts.md)은 GPU 리소스를 OS 핸들로 전달합니다. 이 경로는 Opale 뷰포트 v1의 지원 대상에서 분리한 플랫폼 어댑터입니다. 아래는 해당 어댑터의 리소스와 동기화 접점입니다.

| 플랫폼 | 메모리와 동기화 접점 |
|---|---|
| Apple | IOSurface와 프로세스 간 공유 가능한 Metal 이벤트 및 단조 증가 값. [IOSurface](https://developer.apple.com/documentation/iosurface) · [MTLSharedEvent](https://developer.apple.com/documentation/metal/mtlsharedevent) |
| Windows D3D12 | 같은 어댑터에서 열 수 있는 공유 리소스와 공유 fence 핸들·값. [CreateSharedHandle](https://learn.microsoft.com/en-us/windows/win32/api/d3d12/nf-d3d12-id3d12device-createsharedhandle) |
| Linux | dma-buf FD와 포맷·modifier·plane별 offset/stride, 명시적 acquire/release sync-file FD. [커널 dma-buf](https://docs.kernel.org/driver-api/dma-buf.html) |
| Android | AHardwareBuffer와 네이티브 acquire/release fence FD. [Android NDK 하드웨어 버퍼](https://developer.android.com/ndk/reference/group/a-hardware-buffer) · [네이티브 fence](https://registry.khronos.org/EGL/extensions/ANDROID/EGL_ANDROID_native_fence_sync.txt) |

양쪽이 형식·디바이스 호환성과 동기화 기능을 확인한 경우에만 해당 경로를 엽니다. 실제 핸들은 OS 전송으로 넘기고 제어 프레임에는 슬롯·세대·프레임 순번과 메타데이터만 싣습니다. 생산 완료 신호 뒤에만 읽고 소비 완료 신호 뒤에만 같은 버퍼를 덮어씁니다. 연결 종료·세대 교체 시 각 프로세스가 소유한 핸들을 닫습니다. 기능·권한·동기화 계약을 충족하지 못하면 CPU 슬롯으로 전환합니다. 플랫폼별 수입·동기화는 실제 두 프로세스에서 확인해야 합니다.

## 2. 호환 어댑터 및 플랫폼 바인딩

| 분류 | 크레이트 / 접점 | 역할 및 준수 계약 |
|---|---|---|
| **Web DOM & 적응형 UI** | `serge-web-dom`, `serge-web-ionic` | 모든 DOM/Web API 표현·출력 및 Ionic 레퍼런스 기반 모바일·데스크톱 적응형 웹 컴포넌트 |
| **Web 로컬 데브 서버** | `serge-web-dev` (`hyve dev`) | 최소 **CRA(Create React App) 수준**의 내장 데브 서버·WASM 번들링·상태 보존 HMR |
| **Native 데스크톱 어댑터** | `serge-platform-desktop` | macOS/Windows/Linux 멀티 윈도우·네이티브 메뉴바/MenuBarExtra·트레이·단축키·VSync·IME·독/작업표시줄 진행률 연동 |
| **Native iOS / Android 어댑터** | `serge-platform-ios`, `serge-platform-android` | `CADisplayLink`/`Choreographer` 프레임 동기화, Safe Area·키보드 회피, Android PiP·폴더블 힌지(Posture) 연동. **iOS Dynamic Island/WidgetKit(`#[ios::live_activity]`) 및 Android 위젯/Ongoing Notification**은 OS 제약상 커스텀 GPU 렌더링 대신 `hyve build` 시점의 정적 `ui!` 서브셋 → SwiftUI/Glance 코드젠 브리지로 동작 |
| **Native HW · SW · Alert SDK** | `serge-native-sys` | `serge::hw`(센서 링 버퍼/시그널, 카메라 `ExternalTexture` 제로카피, 마이크 링 버퍼, 생체인증, 펜슬/햅틱, BLE) 및 `serge::sys`(OS 네이티브 Alert/ActionSheet/권한 팝업, 파일 피커, Keychain, 공유 시트, 백그라운드 태스크, `AccessKit` 접근성) |
| **3-Tier 통신 어댑터** | `serge-adapter-client` | ① **Tier 1 인프로세스**(`Signal<T>` + Lock-Free `SharedRingBuf`), ② **Tier 2 로컬 IPC**(OS `shm`/`mmap` Shared Ring Buffer — `#[repr(C)]` POD·찢어짐 방지 스냅샷·App Group 샌드박스 준수 + Unix Socket/Named Pipe `hyve-serde` 바이너리 RPC), ③ **Tier 3 원격 서버**(Dual-Rail: 고속 레일은 `hyve-serde` 바이너리를 HTTP 본문·WebTransport로 나르고 차단 시 같은 프레임을 WS로 운반, 표준 레일은 HTTP/JSON+WS. `?ticket=` 핸드셰이크 및 폴백 `seq` 연속 복구) 통합 실행 및 뷰 언마운트 시 구조적 취소 전파 |
| **옵트인 웹뷰 어댑터** | `serge-adapter-webview` | 네이티브 UI는 기본적으로 웹뷰를 절대 쓰지 않으나, 네이티브 앱 일부 슬롯에 웹 화면을 띄워야 할 때 **같은 레포의 `serge-web-dom` 산출물을 네이티브 웹뷰 서피스로 마운트하고 `hyve-serde` IPC로 상태를 동기화하는 내부 옵트인 어댑터** |
| **프로덕션 웹 서빙 접점** | `orbic::orbic-adapter-serge` | `serge` 웹 타깃의 프로덕션 서빙 및 SSR 호스팅은 `orbic`이 담당 (§3 참조) |

### 2.1 `serge-adapter-client`의 Phase 1 범위 (제안, 2026-11-24)

- **Tier 1 인프로세스**: 시그널과 `SharedRingBuf` 연결에 더해, `hyve-api`의 전송 트레이트를 구현한 인프로세스 전송(`orbic`이 제공)을 그대로 받아 쓸 수 있습니다. `serge`는 `orbic`에 의존하지 않고 트레이트만 봅니다.
- **Tier 3 원격 요청-응답**: `hyve/docs/03-adapters-and-pipeline.md` §3의 전송 계약을 구현한 원격 전송입니다. 기준 URL(출처와 경로 접두어)과 레일 고정 여부를 설정으로 받습니다.
- **자격 증명**: 웹에서는 브라우저가 세션 쿠키를 보내도록 요청하고, 앱이 세션 응답에서 받은 CSRF 토큰을 전송에 설정하면 상태를 바꾸는 호출에 `X-CSRF-Token`으로 붙입니다. 네이티브에서는 앱이 설정한 `Bearer` 토큰을 붙입니다. Phase 1의 전송은 받은 토큰을 메모리에만 두고, 토큰을 어디에 보관할지는 앱이 정합니다. OS 보안 저장소 연동은 `serge-native-sys`와 함께 Phase 2에 옵니다. 로그인 화면과 세션을 얻는 호출은 앱이 만듭니다.
- **개발 서버**: 웹 번들은 `serge-web-dev`가, API는 `orbic` 개발 서버가 서빙하므로 출처가 다릅니다. `serge-web-dev`는 `hyve.toml`의 `[serge] api_proxy`에 적은 경로 접두어를 백엔드로 프록시해 브라우저에서는 같은 출처로 보이게 합니다.
  - **형식 (제안, 2026-11-25)**: `api_proxy = "http://127.0.0.1:3000/v1"`처럼 URL 하나이고, 그 경로(`/v1`)가 프록시할 접두어입니다(경로가 없으면 `/v1`). `hyve-serge dev --api-proxy <URL>`이 설정보다 우선합니다. 백엔드에 닿지 못하면 `502`와 이유를 돌려줍니다.
- **구현 세부 (제안, 2026-11-25)**: 원격 전송은 `RemoteTransport::new(기준 URL)`이고 `.rail(..)`, `.bearer(..)`, `.csrf_token(..)`, `.header(..)`, `.root_certificates(..)`로 설정합니다. 고속 레일이 `415`를 받으면 그 호출을 표준 레일로 한 번 다시 보냅니다. 네이티브는 `serge`의 `tokio` 런타임에서 `hyper`로 보내고, `https://`는 `rustls`(암호 구현은 `ring`을 명시적으로 넘겨 프로세스 기본값에 기대지 않음)로 보냅니다. 서버 인증서는 운영체제의 신뢰 저장소로 검증하고(`rustls-platform-verifier`), `.root_certificates(PEM)`를 주면 그 CA만 신뢰합니다(개발 서버의 자체 CA). 인증서 검증을 끄는 옵션은 두지 않습니다. 인증서 오류는 `transport_unavailable`이고 표준 레일로 다시 보내지 않습니다. `http://`는 TLS를 쓰지 않습니다(2026-11-25). 웹은 `fetch`와 `AbortController`로 보내며 `credentials: include`로 세션 쿠키를 함께 보냅니다. 뷰가 언마운트되거나 호출의 토큰이 취소되면 요청을 끊고 `cancelled` 오류를 돌려줍니다. `SharedRingBuf`는 `hyve-core`의 락 없는 링(슬롯마다 시퀀스 락, 쓰는 쪽은 읽는 쪽을 기다리지 않음)을 감싸고 캔버스용 `FrameSource`를 더한 것이며, 샘플은 `#[repr(C)]` POD(`Pod + Zeroable`)입니다. `read_latest_snapshot`이 찢어지지 않은 최신 샘플을 복사합니다. 사용자는 `#[derive(serge::bytemuck::Pod, serge::bytemuck::Zeroable)]`과 `#[bytemuck(crate = "serge::bytemuck")]`로 샘플 타입을 만들어, 원본 크레이트를 직접 참조하지 않습니다(2026-11-25).

`serge`의 `shm` 피처를 켜고 `SharedRingBuf::<Sample>::new_shared(capacity, layout_id)`로 프로세스 간 링을 만듭니다. 송신자는 앱 전용 Unix 소켓으로 `ring.send(&socket)`을 호출하고 수신자는 같은 레이아웃 ID로 `SharedRingBuf::<Sample>::receive(&socket, layout_id)`를 호출합니다. Linux는 크기가 봉인된 memfd, macOS는 이름을 즉시 해제한 POSIX 공유 메모리를 사용합니다. 고정 크기·소유 UID·접근 모드·POD 크기와 정렬을 검사합니다. 독자는 프레임에서 공유 리비전을 읽어 다른 프로세스가 발행한 샘플로 캔버스를 갱신합니다. 버퍼는 마지막 디스크립터와 매핑이 닫힐 때 해제됩니다. [캔버스 통합](../crates/serge-adapter-client/src/ring.rs)

`ipc` 피처의 `RemoteTransport::local(endpoint, "/v1")`는 같은 사용자의 Unix 소켓이나 Windows 명명된 파이프로 서버 함수를 호출합니다. 서버는 `orbic::ipc::Listener::bind(endpoint)`를 `orbic::serve`에 넘깁니다. 두 레일의 직렬화, 가드, 요청 ID와 취소는 기존 HTTP/1.1 어댑터를 거칩니다. Unix에서는 사용자 소유 전용 디렉터리와 커널 피어 UID를 확인하고 Windows에서는 현재 사용자 SID로 파이프 ACL을 만들고 연결에서 서버의 SID를 확인합니다. 플레이그라운드는 양 프로세스에 같은 `SERGE_PLAYGROUND_IPC`를 주면 이 경로를 사용합니다. [별도 프로세스 RPC 테스트](../apps/serge-playground/tests/local_rpc.rs)


- **도구 단계**: `hyve dev`와 `hyve build`가 호출하는 웹 번들링·개발 서버·네이티브 실행은 `serge`가 제공하는 `hyve-serge` 실행 파일의 단계입니다(`hyve/docs/01-dx-and-dsl.md` §2).
  - **함께 띄우기 (제안, 2026-11-25)**: `hyve dev`는 `hyve-serge dev`와 `hyve-orbic dev`(패키지가 `[package.metadata.orbic.server]`로 선언한 백엔드를 빌드·실행하고 소스 변경 시 재기동, `orbic/docs/01-dx-and-dsl.md` §3)를 동시에 띄우고, 표준 입력 파이프가 닫히면 둘 다 자기가 띄운 서버와 창을 정리하고 끝납니다(진행 중이던 `cargo build`는 끝까지 돎). 어느 쪽이든 끝나면(예: 창을 닫음) 개발 세션이 끝나므로, `hyve-serge stages`는 앱(`[package.metadata.serge] web = true`)이 있을 때만 `dev`를 답합니다. `hyve-serge`는 네이티브 앱을 `cargo build`한 뒤 만들어진 실행 파일을 직접 실행해(`cargo run`을 거치지 않음) 멈출 때 창이 남지 않습니다. 웹 데브 서버는 백엔드 없이도 뜨고, 백엔드가 아직 없으면 프록시가 `502`를 돌려줍니다.

<a id="bundle-v1"></a>

## 3. 프로덕션 번들과 서빙 v1 (Phase 2 설계 확정, 2026-11-26)

`hyve-serge build`가 프로덕션 번들 루트에 UTF-8 JSON `serge-bundle.json`을 씁니다. 정적 호스트는 이 파일과 산출물만 읽습니다. SSR을 사용하는 호스트는 `serge-web-ssr`의 트레이트도 링크합니다. 개발 서버는 `build_web`으로 소스 번들을 만듭니다. 프로덕션 빌드는 해시 에셋과 빌드별 페이지를 검증한 뒤 매니페스트를 원자적으로 교체합니다. [현재 Bundle](../crates/serge-web-dev/src/bundle.rs)

**`SergeBundleManifest` v1**의 필수 필드는 아래와 같습니다. JSON은 camelCase이며 같은 버전에서 알 수 없는 부가 필드는 건너뜁니다. 미지원 버전·모드나 필수 필드 누락은 마운트 오류입니다. 파일명은 URL의 마지막 세그먼트에서 ASCII 영숫자와 `._~-`만 사용하며 `.`·`..`는 거부합니다. JSON 형태 검증 후 아래 마운트 검증을 수행합니다. [기계 판독용 JSON Schema](contracts/serge-bundle-v1.schema.json)

| 필드 | 타입과 의미 |
|---|---|
| `version`, `buildId` | 정수 `1`, 빌드마다 새로 발급한 ULID 문자열. 번들·SSR 핸들러·하이드레이션 부트스트랩이 같은 값을 갖습니다. |
| `assets[]` | 공개할 파일마다 `{url, file, mediaType, bytes, sha256}`. `url`은 `/assets/<sha256>/<이름>`이며 `sha256`은 파일 바이트의 소문자 64자리 16진수, `bytes`는 0 이상의 정수, `file`은 번들 루트 상대 경로입니다. JS/Wasm이 참조하는 URL도 이 목록과 일치해야 합니다. |
| `routes[]` | 라우터 선언 순서의 `{pattern, mode, file?}`. `csr`은 앱 셸 HTML, `ssg`는 빌드 시 열거한 구체 경로별 HTML로 `file`이 필수입니다. `ssr`은 핸들러로 렌더하며 `file`은 허용하지 않습니다. `csr`·`ssr`은 Serge의 전체 세그먼트 `{name}` 패턴을 씁니다. |

생산자와 소비자가 사용하는 Rust 값은 다음 형태입니다. JSON의 `mode` 태그는 `SergeRoute`의 변형을 선택합니다. 파일 길이는 JSON 정수로 정확히 전달할 수 있는 `0..=2^53-1`로 제한합니다. [JSON의 정수 상호운용 범위](https://www.rfc-editor.org/rfc/rfc8259.html#section-6)

```rust
pub struct SergeBundleManifest {
    pub version: u16,
    pub build_id: String,
    pub assets: Vec<SergeAsset>,
    pub routes: Vec<SergeRoute>,
}
pub struct SergeAsset {
    pub url: String,
    pub file: String,
    pub media_type: String,
    pub bytes: u64,
    pub sha256: String,
}
pub enum SergeRoute {
    Csr { pattern: String, file: String },
    Ssg { pattern: String, file: String },
    Ssr { pattern: String },
}
```

예를 들어 `pages/home.html` 하나만 있는 SSG 산출물은 다음과 같습니다.

```json
{
  "version": 1,
  "buildId": "01ARZ3NDEKTSV4RRFFQ69G5FAV",
  "assets": [],
  "routes": [{"pattern": "/", "mode": "ssg", "file": "pages/home.html"}]
}
```

**마운트.** Orbic은 새 번들을 별도 루트에서 전부 검증한 뒤 한 번에 교체합니다. 파일 경로는 정상 상대 경로만 허용하고 심볼릭 링크·상위 경로·공개 에셋과 라우트 HTML의 별칭을 거부합니다. 에셋 길이와 해시(URL·파일·필드 일치), MIME 타입, HTML 존재, 중복 URL·패턴, 에셋·API·관리 접두어를 직접 선언한 UI 라우트의 충돌, SSR 핸들러 존재와 `buildId` 일치를 확인합니다. 실패하면 이전 번들을 계속 제공합니다. 진행 중 응답은 시작한 번들을 끝까지 참조합니다. 새 배포 직전에 열린 페이지가 청크를 받을 수 있도록 현재 및 직전 성공 번들의 검증된 공개 해시 에셋을 제공합니다. 그보다 오래된 페이지에서 에셋이 사라지면 현재 URL을 한 번 다시 로드합니다.

**요청.** Orbic의 API·관리 경로가 먼저입니다. 예약 접두어와 그 아래 경로는 해당 어댑터가 끝까지 처리합니다. 예를 들어 미등록 `/v1/...`와 `/assets/...`는 각각 API와 에셋의 404입니다. 나머지 요청은 선언 순서상 처음 일치하는 UI 라우트로 갑니다. 정적 파일은 목록으로만 접근하고, HTML은 라우트 가드를 통과한 뒤에만 제공합니다. 보호된 사용자 데이터를 SSG에 굽지 않습니다. UI 경로의 GET·HEAD를 처리하고, 일치하는 UI 경로의 다른 메서드는 405와 `Allow: GET, HEAD`, 미등록 경로는 404입니다. 요청 경로를 디스크 경로에 이어 붙이지 않습니다. [현재 라우터의 패턴과 탐색 순서](../crates/serge-router/src/lib.rs)

**라우트 권한.** 마운트할 때 앱이 Orbic의 컴포넌트 가드를 UI 경로 그룹에 연결하거나 해당 그룹을 공개로 선언합니다. 호스트는 모든 매니페스트 라우트가 선언된 정책에 속하는지 검사합니다. 번들의 클라이언트 측 리다이렉트는 서버 권한을 부여하지 않습니다. SSR 요청 스코프는 해당 가드를 통과한 인증 주체와 취소 토큰을 상속합니다.

**캐시.** 공개 해시 에셋은 `public, max-age=31536000, immutable`과 SHA-256 ETag를 사용합니다. 이 TTL은 URL이 내용에 묶인 에셋에 정한 제품 기본값입니다. 공개 정적 HTML은 `no-cache`로 재검증하고 보호된 HTML·요청별 SSR은 `private, no-store`로 응답합니다. HEAD·조건부 요청도 먼저 같은 권한을 검사합니다. HEAD는 본문을 전송하지 않습니다. [HTTP 의미론](https://www.rfc-editor.org/rfc/rfc9110.html#section-9.3.2) · [HTTP 캐싱](https://www.rfc-editor.org/rfc/rfc9111.html) · [불변 응답](https://www.rfc-editor.org/rfc/rfc8246.html)

앱의 `[package.metadata.serge]`에서 `export`에 `SergeApp::run()`을 호출하는 바이너리 이름을, `ssg`에 빌드할 공개 경로 목록을, `ssr`에 서버 렌더링할 라우트 패턴 목록을 지정합니다. 빌더가 앱의 실제 라우터에서 선언 순서와 가드를 읽습니다. 가드가 있는 화면을 SSG로 지정하면 빌드가 실패합니다. 플레이그라운드는 홈을 SSG로 만들고 직접 접근한 `/projects`에는 서버 세션 가드를 적용합니다. 로컬에서 재현하려면 `CHROMEDRIVER=/path/to/chromedriver python3 apps/serge-playground/tests/browser/production.py`를 실행합니다. [프로덕션 빌더](../crates/serge-web-dev/src/production.rs) · [Chrome 통합 테스트](../apps/serge-playground/tests/browser/production.py)

<a id="ssr-v1"></a>

### 3.1 `SergeSsrHandler` v1

다음은 `serge-web-ssr`가 소유하고 `orbic-adapter-serge`가 소비할 네이티브 Rust 시그니처입니다. 헤더를 결정하는 Future와 본문 Stream을 분리합니다.

```rust
use std::{future::Future, pin::Pin};
use bytes::Bytes;
use futures_core::Stream;
use hyve_core::{HyveResult, RequestId, Scope};

pub struct SsrRequest {
    pub path: String,
    pub query: String,
    pub request_id: RequestId,
    pub scope: Scope,
}

pub type SsrBody = Pin<Box<dyn Stream<Item = HyveResult<Bytes>> + Send + 'static>>;
pub enum SsrResponse {
    Html { status: u16, body: SsrBody },
    Redirect { location: String, permanent: bool },
}
pub type SsrFuture = Pin<Box<dyn Future<Output = HyveResult<SsrResponse>> + Send + 'static>>;

pub trait SergeSsrHandler: Send + Sync {
    fn build_id(&self) -> &str;
    fn render(&self, request: SsrRequest) -> SsrFuture;
}
```

Orbic은 매칭한 경로(쿼리 제외), 원본 쿼리(`?` 제외), 요청 ID와 권한 검사 후의 요청 스코프를 넘깁니다. 쿠키·TLS·소켓 타입은 경계 밖에 둡니다. 요청별 Serge 시그널·DOM처럼 `!Send`인 값은 한 실행 스레드에서 생성·사용·해제하고, 소비자가 읽을 때만 진행하는 스트림 또는 유한 용량 채널로 소유한 바이트 청크를 넘깁니다. 연결이 끊기면 Orbic은 요청 스코프를 취소하고 Future·본문을 drop합니다. 생산자도 스코프 취소나 수신자 종료를 관찰해 중단합니다. [Hyve 요청 스코프와 취소](https://github.com/hyperish/hyve/blob/main/crates/hyve-core/src/scope.rs)

`Html`은 200 또는 404와 UTF-8 HTML을, `Redirect`는 같은 출처의 루트 상대 경로와 307(임시)/308(영구)을 뜻합니다. 호스트는 목적지를 URL로 해석해 출처를 확인하고 `//`로 시작하는 값·역슬래시·제어문자를 거부합니다. 헤더를 보내기 전 오류는 Orbic이 요청 ID를 담은 공개 오류로 변환합니다. 전송을 시작한 뒤에는 스트림을 실패로 종료하고 요청 ID를 기록합니다. HEAD는 같은 권한·상태·헤더를 계산한 후 본문 생산을 종료합니다.

Serge는 하이드레이션에 필요한 공개 상태만 `hyve-serde`로 인코딩해 비실행 데이터 블록에 base64로 싣습니다. 부트스트랩은 빌드 ID와 모델/코덱 지문을 확인하고 바이트 검증을 통과한 값만 복원합니다. 오래되거나 손상된 상태는 버리고 같은 URL을 CSR로 새로 마운트합니다. 입력 이벤트나 서버 변경 요청을 자동 재생하지 않습니다. [rkyv 검증](https://rkyv.org/validation.html)

<a id="webview-v1"></a>

## 4. 옵트인 웹뷰 동기화 v1 (Phase 3 설계 확정, 2026-11-26)

Android는 Cargo의 `[package.metadata.android] webview = true`와 `serge/webview`를 켜고 NativeActivity의 물리 픽셀 영역에 `webview::Slot::attach`로 시스템 WebView를 붙입니다. 슬롯별 HTTPS 출처에서 해시가 검증된 문서와 자산을 제공하고 AndroidX WebKit 1.12.1로 최상위 문서의 메시지만 받습니다. 마지막 핸들이 해제되면 해당 뷰와 메시지 수신을 종료합니다. [Android 슬롯](../crates/serge-adapter-webview/src/android.rs) · [AndroidX WebViewCompat](https://developer.android.com/reference/androidx/webkit/WebViewCompat)

`serge-adapter-webview`를 선택한 앱에서 호스트는 슬롯의 상태와 이벤트 결과를 소유합니다. 웹뷰는 검증된 로컬 번들의 최상위 문서 하나를 마운트합니다. 외부 탐색·새 창·하위 프레임·외부 스크립트는 브리지에서 차단합니다. 문서 전환 시작 시 이전 브리지를 폐기하며 슬롯과 문서 출처를 네이티브에서 확인합니다. 이 격리를 보장할 수 없는 플랫폼에서는 브리지를 열지 않습니다. Wry는 플랫폼에 따라 iframe의 요청 URL 대신 메인 프레임 URL을 전달하므로 URL 검사 하나에 의존할 수 없습니다. [Wry IPC 핸들러의 플랫폼별 제약](https://docs.rs/wry/0.58.0/wry/struct.WebViewBuilder.html#method.with_ipc_handler)

**버전과 레일 협상.** `Hello`와 `Ready`는 먼저 UTF-8 JSON 객체로 교환합니다. 공통 키는 `version`, `kind`, `buildId`, `session`, `generation`, `fingerprint`입니다. 지문은 16자리 소문자 16진수입니다. 호스트의 로컬 번들과 빌드 ID가 다르면 그 번들을 다시 마운트합니다. 프로토콜 v1을 양쪽이 이해하면, `Ready.rail`은 지문이 같을 때 `rkyv`, 다를 때 `json`입니다. 서로 지원하는 프로토콜 버전이 없으면 입력을 중지하고 버전을 표시합니다. [공통 레일 호환 규칙](https://github.com/hyperish/docs/blob/main/docs/02-core-contracts.md)

**프레임.** 고속 레일은 표준 base64로 인코딩한 바이너리 프레임 하나를 문자열 IPC로 보냅니다. 앞 4바이트는 ASCII `SGWV`, 다음은 `version: u16 LE = 1`과 봉투 지문 `u64 LE`입니다. 네이티브와 웹의 Wasm이 버전·지문을 확인한 뒤 `hyve-serde`의 정렬·`bytecheck` 검증을 거쳐 본문을 읽습니다. 표준 레일은 같은 메시지를 `hyve-serde` JSON 객체로 보내며 `kind`는 소문자로 시작하는 camelCase 이름, 봉투의 세대·순번은 정규 10진 문자열입니다. 이 표현을 핸드셰이크에서도 씁니다. 잘못된 아카이브나 JSON은 실행하지 않습니다. 재협상에도 세션·순번·이벤트 ID를 유지합니다. 초기 상한은 바이너리 프레임 또는 JSON UTF-8 본문 64 KiB이며, base64 길이도 할당 전에 제한합니다. 큰 데이터는 권한과 수명이 묶인 별도 참조로 전달합니다. [Hyve 코덱](https://github.com/hyperish/hyve/blob/main/crates/hyve-serde/src/lib.rs) · [rkyv 검증](https://rkyv.org/validation.html)

**주소와 순서.** 모든 본문에는 `session`(인증 주체와 슬롯에 묶인 호스트 발급 ULID), `generation: u64`(문서를 열 때마다 증가), `kind`가 있습니다. `session`은 식별자입니다. 호스트는 이벤트마다 현재 권한도 검사합니다. 상태 순번과 이벤트 순번은 세션 안에서 방향별로 독립적이며 `u64` 1부터 시작합니다. 0은 수신 이력 없음입니다. 이벤트 `id`는 생성 시 발급한 ULID입니다. 같은 요청을 다시 보낼 때 유지합니다. 순번과 큰 정수는 JS `Number`를 거치지 않습니다.

핸드셰이크 후 데이터 종류는 `Snapshot(stateSeq, state, lastEvent)`, `AckState(stateSeq)`, `Event(eventSeq, id, payload)`, `EventResult(eventSeq, id, outcome)`, `Resync(lastStateSeq)` 순서로 바이너리 태그 0~4를 사용합니다. `lastEvent`는 없거나 `(eventSeq, id, outcome)`이며 `outcome`은 진행 중, 성공 값, 공개 오류 중 하나입니다. 두 끝은 같은 어댑터의 봉투 타입을 사용하며 지문에 앱의 상태·이벤트·결과 모델과 코덱 버전을 포함합니다.

| 메시지 | 수신자가 할 일 |
|---|---|
| 호스트 → `Hello`, 문서 → `Ready` | 새 세대와 빌드 ID를 교환합니다. 로컬 번들의 빌드 ID와 새 세대가 일치하고 레일 협상이 끝나야 프레임을 받습니다. 호스트는 보관한 마지막 이벤트 순번·ID·결과(또는 진행 중)와 현재 상태 전체를 보냅니다. |
| 호스트 → `Snapshot(stateSeq, state, lastEvent)`, 문서 → `AckState(stateSeq)` | 최초·재동기화 스냅샷은 그 순번을 기준점으로 세웁니다. 정상 전달은 직전 +1이며 적용한 뒤 ACK합니다. 중복은 다시 ACK하고, 간격이 벌어지면 `Resync`로 현재 전체 스냅샷을 요청합니다. 미확인 스냅샷은 하나만 두고 그 사이의 상태 변경은 다음 스냅샷으로 합칩니다. 상태 관찰에서 개별 편집 이벤트를 추론하지 않습니다. |
| 문서 → `Event(eventSeq, id, payload)`, 호스트 → `EventResult(eventSeq, id, result)` | 문서는 같은 출처의 탭 세션 저장소에 비밀이 없는 미확인 이벤트 하나를 기록한 뒤 보냅니다. 호스트는 예상 순번만 수락하고, 실행 전에 ID·내용을 진행 중으로 기록합니다. 같은 순번·ID·내용의 재전송에는 진행 상태 또는 보관한 결과를 회신합니다. 동일 ID에 다른 내용, 건너뛴 순번은 실행하지 않고 재동기화합니다. 권한 거부를 포함한 최종 결과까지 순번을 소비합니다. |
| 리로드·종료 | 문서는 `Ready` 후 전체 상태를 적용한 다음 보관한 이벤트를 대조합니다. 이미 완료면 결과만 반영하고, 미수신이면 **원래 순번·ID**를 새 세대에 다시 보냅니다. 진행 중이면 결과를 기다립니다. 호스트는 다음 이벤트를 수락할 때까지 직전 결과를 유지합니다. 이전 세대의 프레임·ACK는 무시합니다. |

이벤트 처리 중에는 입력 제출을 일시 중지하고 `busy`를 표시합니다. 상태 갱신과 달리 이벤트는 덮어쓰지 않습니다. 호스트 재시작·주체 변경·순번 소진은 새 세션을 요구합니다. 이전 세션의 대기 이벤트는 자동 전송하지 않으며, 결과가 불명인 외부 쓰기는 앱의 영속 멱등 키와 상태 조회로 조정합니다. 재전송 가능한 이벤트는 원문을 안전하게 보관할 수 있는 작업으로 제한합니다. 자격 증명이나 민감한 입력은 저장하지 않고 호스트가 소유한 단발 작업으로 처리합니다. [공통 취소·부수효과 계약](https://github.com/hyperish/docs/blob/main/docs/02-core-contracts.md)

ACK·결과가 유실되면 같은 세대에서 같은 순번·내용으로 재전송합니다. v1의 기본 응답 대기는 1초이고 최대 세 번 재전송한 뒤 입력을 멈추고 새 세대로 핸드셰이크합니다. 재접속에도 실패하면 연결 끊김을 표시하고 대기 이벤트를 유지합니다. 이는 로컬 브리지의 유실 감지와 재시도 횟수에 정한 기본값입니다. 진행 중 응답은 실행을 다시 시작하지 않으며, 호스트가 완료 결과를 현재 세대로 전달합니다.


호스트는 `VerifiedBundle`과 HTML 해시로 `LocalDocument::from_bundle`을 호출하고, `WindowHandle::with_native` 안에서 `Slot::attach`에 현재 상태·권한 함수·이벤트 핸들러를 전달합니다. 로컬 문서의 `sergeWebview.connect({buildId, onState, onResult, onStatus})`가 첫 스냅샷을 받으면 `sergeWebview.submit(payload)`로 작업을 제출할 수 있습니다. 핸들러는 `slot.update(state)`와 `slot.finish(event, result)`로 결과를 보냅니다. WASM 문서는 `codec: {encode, decode}`와 모델 지문을 함께 넘깁니다. Linux의 X11 창에는 WebKitGTK 4.1과 GTK 3이 필요합니다. [실행 중인 호스트 예제](../apps/serge-playground/tests/webview.rs) · [문서 스크립트](../apps/serge-playground/tests/browser/webview.js)

### 모바일 위젯 전달

`package.metadata.serge.widgets`에 선언 파일 경로를 배열로 두고 Android용 `android_package`를 설정합니다. `hyve build`가 정적 선언을 검사하고, `--format apk,aab`는 Gradle로 실제 앱의 리소스·NativeActivity와 생성된 Android 라이브러리를 병합합니다. 위젯이 있는 앱의 Gradle 실행 파일은 `HYVE_GRADLE`로 지정할 수 있습니다. 반환된 `PlatformWidget`의 `kind`와 `state_json`을 `AndroidSystem::update_widget`에 넘기면 지정한 Glance 인스턴스나 해당 종류 전체를 갱신합니다. 진행 알림은 `ongoing_notification`과 `cancel_notification`으로 게시·교체·취소합니다. [정적 선언](../apps/serge-playground/src/platform_widgets.rs) · [Android 위젯](https://developer.android.com/develop/ui/compose/glance/configuration) · [알림 API](https://developer.android.com/reference/android/app/Notification.Builder)

생성한 iOS 앱에서 `--format ios-sim` 또는 `ipa`는 WidgetKit 확장을 빌드하고 호스트 서명 전에 앱의 `PlugIns`에 포함합니다. 홈 위젯은 `ios::publish_widget`로 App Group의 모델을 원자적으로 교체하고 타임라인을 갱신합니다. Shortcuts의 읽기 동작은 이 상태를 반환합니다. Live Activity는 `ios::start_activity`가 돌려준 ID로 `update_activity`와 `end_activity`를 호출합니다. 홈 위젯의 App Group 기본 식별자는 `group.<앱 번들 ID>`이며, `[package.metadata.ios] widget_app_group`에서 지정할 수 있습니다. 기기 서명 시 앱과 확장 모두 그 그룹 권한이 필요합니다. [WidgetKit 타임라인](https://developer.apple.com/documentation/widgetkit/timelineprovider) · [App Intent 인증 정책](https://developer.apple.com/documentation/appintents/intentauthenticationpolicy/requiresauthentication)

### 모바일 기기 입력과 창

Android API 30 이상에서는 시스템 표시줄, 화면 컷아웃, 화면 키보드가 차지하는 영역을 `WindowInsets`에서 읽어 네이티브 콘텐츠 좌표로 변환합니다. 같은 영역이 렌더링, 터치, 접근성에 적용됩니다. Pixel 6 인수 테스트는 상단의 카운터 링크를 터치한 뒤 프로젝트 저장과 재조회를 검사합니다. [Android 창 처리](../crates/serge-platform-desktop/src/android_access.rs) · [터치 인수 테스트](../apps/serge-playground/tests/android/projects.py)

Android 앱의 `AndroidSystem::window_posture`는 Jetpack에서 받은 창 기준 접힘 영역, 차폐 및 분리 상태를 제공합니다. 일반 화면은 빈 목록을 반환합니다. `SensorKind::HingeAngle`은 센서가 있는 기기에서 각도를 도 단위로 제공합니다. `AndroidSystem::enter_picture_in_picture`는 매니페스트에 PiP 지원을 선언한 Activity를 UI 스레드에서 전환합니다. [플레이그라운드 매니페스트](../apps/serge-playground/android/AndroidManifest.xml) · [에뮬레이터 인수](../apps/serge-playground/tests/android/sdk.py)

Android `hw::CameraTexture`는 호출자의 EGL 컨텍스트에서 Camera2의 외부 OES 텍스처, 타임스탬프와 UV 변환을 제공합니다. Vulkan 1.1에서는 `hw::CameraBufferReader`가 받은 이미지의 AHardwareBuffer를 `hw::vulkan_camera::VulkanCameraImage`로 가져옵니다. 장치에서 외부 버퍼·외부 큐 패밀리·YCbCr 변환을 켜고 acquire와 release를 기록한 뒤 마지막 GPU 사용이 끝날 때까지 이미지를 유지합니다. Apple `MetalCamera`는 Core Video 버퍼를 wgpu 텍스처로 가져오며, GPU가 사용을 마칠 때까지 IOSurface를 유지합니다. Apple 마이크는 앱의 `NSMicrophoneUsageDescription`과 OS 승인을 확인합니다. iOS 앱은 입력을 허용하는 오디오 세션을 설정한 뒤 `hw::Microphone::open`으로 단일 채널 PCM 링을 엽니다. [Metal 카메라](../crates/serge-native-gpu/src/camera.rs) · [AVAudioEngine 입력 노드](https://developer.apple.com/documentation/avfaudio/avaudioengine/inputnode) · [Android 기기 입력](../crates/serge-native-sys/src/android/)

iOS는 `IosSystem::for_window`로 창을 지정하거나 활성 창이 하나일 때 `foreground`로 선택합니다. 이 핸들은 UIKit의 Alert·Action Sheet, 공유 시트, 클립보드, 햅틱과 Application Support 경로를 제공합니다. 백그라운드 실행 핸들을 해제하거나 OS가 만료를 알리면 해당 작업을 종료합니다. Android는 매니페스트의 VIEW 필터로 받은 URI를 `launch_uri`로 읽고 `vibrate_waveform`으로 진동과 일시 정지를 조합합니다. Android 앱은 Cargo의 `[package.metadata.android]`에서 `background_task = true`를 켜고 앱이 보일 때 `AndroidSystem::background_task`를 호출합니다. 동시에 진행하는 작업은 화면에 표시하는 알림을 공유하며, 첫 요청으로부터 최대 165초 안에 끝납니다. Android가 서비스를 중단하거나 마지막 핸들이 해제되면 연장도 끝납니다. [UIKit](../crates/serge-native-sys/src/ios.rs) · [Android](../crates/serge-native-sys/src/android.rs) · [Android 단기 서비스](https://developer.android.com/develop/background-work/services/fgs/service-types#short-service)

데스크톱의 `system::set_task_progress(window, desktop_entry, percent)`는 0~100의 진행률을 표시하고 `None`으로 해제합니다. macOS는 앱 Dock 타일의 배지를, Windows는 해당 창의 작업표시줄 버튼을 갱신합니다. Linux에서는 설치된 `.desktop` 파일명을 넘기면 지원 런처가 세션 D-Bus 신호를 받습니다. [플랫폼 구현](../crates/serge-platform-desktop/src/system/progress.rs) · [AppKit 인수](../apps/serge-playground/tests/system_menu.rs) · [D-Bus 인수](../apps/serge-playground/tests/system_x11.rs)

Android와 iOS에서 `stylus_pad(StylusArea)`로 창 콘텐츠 안의 물리 픽셀 영역을 지정합니다. 핸들이 있는 동안 이 영역의 터치는 그리기 표면이 받습니다. `drain()`은 펜·지우개의 접촉 ID, 위치, 필압(0~1), 기울기(화면 법선 기준 라디안), 시각과 단계를 반환합니다. OS가 묶어 보낸 이동 샘플을 순서대로 읽고, 512개 버퍼에서 밀려난 샘플 수를 함께 반환합니다. Android 시스템 입력 디스패처로 펜 떼기·지우개 취소를 검증했습니다. [Android 입력](../crates/serge-native-sys/android/StylusPad.java) · [UIKit 입력](../crates/serge-native-sys/src/ios/stylus.rs) · [Android 펜 축](https://developer.android.com/develop/ui/views/touch-and-input/stylus-input/advanced-stylus-features) · [Apple Pencil 입력](https://developer.apple.com/documentation/uikit/handling-input-from-apple-pencil)

Android 앱은 Cargo의 `package.metadata.android.document_picker = true`로 시스템 파일 선택을 포함합니다. `pick_document`와 `create_document`가 반환한 content URI를 `read_document`나 `write_document`로 열면 앱은 공급자의 파일 디스크립터를 소유합니다. Android DocumentsUI에서 생성·내용 쓰기·다시 선택해 읽기·취소를 검증합니다. iOS의 `IosSystem::pick_document`는 지정된 UTI의 샌드박스 복사본을 가져오고 `export_document`는 기존 파일의 복사본을 내보냅니다. [Android 인수](../apps/serge-playground/tests/android/sdk.py) · [iOS 파일 피커](../crates/serge-native-sys/src/ios/documents.rs)

각 창의 `WindowSpec::placement(WindowPlacement { floating, bounds_file })`와 동적으로 여는 `WindowHandle::open_placed`는 항상 위 표시와 위치 복원을 지원합니다. 복원할 창마다 별도 파일을 지정합니다. 일반 상태의 논리 크기와 모니터상의 물리 위치, 최대화 여부를 기록하며 이전 모니터가 없으면 현재 화면에서 엽니다. [창 배치](../crates/serge-platform-desktop/src/placement.rs) · [프로세스 재시작 인수](../apps/serge-playground/tests/window_placement.rs)

`NotificationCategory`에 등록한 명령은 요청 ID와 명령 ID로 전달됩니다. Android 앱은 `package.metadata.android.notifications = true`를 켜고 알림 권한을 얻은 뒤 `post_local_notification`으로 게시합니다. `take_notification_action`은 시스템이 전달한 명령 하나를 앱 전용 저장소에서 꺼냅니다. Apple 앱은 시작할 때 `AppleNotifications::install`에 전체 카테고리와 콜백을 전달하고 반환된 핸들을 유지합니다. APNs 페이로드와 로컬 알림에서 같은 카테고리를 쓸 수 있습니다. [Android 구현](../crates/serge-native-sys/src/android/notifications.rs) · [Apple 구현](../crates/serge-native-sys/src/apple_notifications.rs) · [시스템 알림창 인수](../apps/serge-playground/tests/android/sdk.py)

iOS의 `IosSystem::pick_date`와 `pick_time`은 지정한 창에 UIKit 선택기를 표시합니다. 선택 결과는 현지 날짜 `YYYY-MM-DD` 또는 24시간제 `HH:MM`입니다. 취소하거나 시트를 내려 닫은 경우를 분리하며 핸들을 해제하면 시트도 닫힙니다. [UIKit 날짜·시간](../crates/serge-native-sys/src/ios/date_time.rs)

지문 인수 테스트는 API 35 테스트 에뮬레이터에 가상 지문 하나를 등록한 뒤 `SERGE_ANDROID_FINGERPRINT`에 그 번호(1~10)를 지정해 실행합니다. 시스템에서 다른 지문이 거부된 것을 확인한 다음 등록된 지문으로 인증하고 새 요청을 뒤로 가기로 닫습니다. [SDK 인수](../apps/serge-playground/tests/android/sdk.py) · [에뮬레이터 지문 입력](https://developer.android.com/studio/run/emulator-console#finger)

macOS에서 `MenuBarPopover::new`에 제목과 앱의 `NSViewController`, 포인트 단위 크기를 전달하고 `DesktopSystem::menu_bar_popover`에 보관하면 상태 버튼으로 팝오버를 열고 닫습니다. Windows의 메뉴 단축키는 해당 창의 Win32 가속기 표로 전달됩니다. Linux X11은 창 안에 GTK 메뉴 막대를 배치하고 해당 높이만큼 앱의 콘텐츠 영역을 조정합니다. Wayland 세션에서 XWayland를 사용할 수 있으면 시스템 UI를 요청한 앱과 `webview` 피처를 켠 앱은 GTK 자식 창에 같은 X11 디스플레이를 사용합니다. `GDK_BACKEND=wayland`는 Wayland 창을 명시적으로 선택합니다. 일반 창과 D-Bus 트레이는 순수 Wayland에서도 동작합니다. GTK 3 인수에서 모든 메뉴 명령이 전달됐고, 창을 닫을 때 `muda`의 단축키 해제 경고가 한 번 기록됐습니다. Linux 파일 선택은 시스템의 XDG 데스크톱 포털을 사용하고 포털이 없는 환경에서는 Zenity가 대신 엽니다. [AppKit 팝오버](../crates/serge-platform-desktop/src/system/popover.rs) · [GTK 메뉴](../crates/serge-platform-desktop/src/system/x11.rs) · [X11 인수](../apps/serge-playground/tests/system_x11.rs) · [Wayland 데스크톱 인수](../apps/serge-playground/tests/linux/wayland.py)

Apple의 `hw::AppleHaptics::open`은 기기의 진동 지원 여부를 확인합니다. `vibrate_waveform`은 Android와 같은 `HapticPulse` 배열을 받습니다. 각 구간의 밀리초 길이와 0~255 세기를 Core Haptics 패턴으로 옮기며 세기 0은 쉼입니다. macOS 프레임워크가 세 구간의 패턴을 읽어 실제 재생 길이 0.4초를 반환하는지 확인했습니다. [Apple 진동 패턴](../crates/serge-native-sys/src/apple_haptics.rs) · [Apple 패턴 형식](https://developer.apple.com/documentation/corehaptics/representing-haptic-patterns-in-ahap-files)

iOS의 `IosDeepLinks`는 앱이 선언한 스킴에 해당하는 URL을 메인 스레드의 라우트 콜백으로 전달합니다. 생성된 winit 앱에서는 시작할 때 `install_application()`으로 등록합니다. 자체 앱 또는 장면 델리게이트를 둔 호스트는 UIKit이 넘긴 URL을 `forward_url`로 전달합니다. 사용자 지정 스킴은 앱의 `CFBundleURLTypes`에 등록합니다. 시스템으로 URL을 보내는 `IosSystem::open_url`은 UIKit의 수락 여부를 돌려줍니다. iOS 웹뷰 인수 앱은 영역 크기를 변경한 뒤 두 레일의 상태 복원·권한 거부를 검사하고 `simctl openurl`로 들어온 프로젝트 URL을 대조합니다. [UIKit 링크](../crates/serge-native-sys/src/ios/links.rs) · [시뮬레이터 인수](../apps/serge-playground/tests/ios/webview.py)
