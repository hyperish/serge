# 02. 아키텍처와 크레이트 구조 (lace)

> 상태: `apps/`·`crates/` 모노레포·`hyper` 코어 `ref`·웹(DOM)+네이티브(GPU/CPU) 통합 구조는 **확정**, 세부 크레이트 분할은 **제안** · 기준일: 2026-11-18

`lace` 레포는 Cargo 워크스페이스 기반의 모노레포(`apps/`, `crates/`)로 구성합니다(`hyperish/docs`의 `05-development-rules.md` §1). 하나의 공통 UI 코어 위에 **Web(DOM/SSR) 렌더 타깃**과 **Native(GPU/CPU) 렌더 타깃**이 대칭으로 올라갑니다.

## 1. 워크스페이스 디렉터리 구조 (제안)

```
lace/
  apps/
    lace-playground/          # 웹(DOM) 및 네이티브(GPU/CPU) 통합 컴포넌트·120fps 쇼케이스 앱
  crates/
    lace/                     # 사용자 진입 크레이트 (단일 의존성 재수출 + prelude)
    lace-core/                # 뷰 트리, 폼(`hyper-validate`), Suspense/ErrorBoundary/낙관적 UI, 테마·i18n, PaintCtx/ScenePrimitive
    lace-macros/              # 공통 UI 문법(QML/Slint + Dioxus rsx! 스타일 `ui!`) proc macro (hyper-parser 참조)
    lace-layout/              # 웹·네이티브 공용 고속 레이아웃(Flexbox/Grid/절대배치) 및 스타일·토큰 계산 엔진
    lace-router/              # 웹(History API) 및 네이티브(네비게이션 스택) 공용 타입 안전 라우터·중첩 레이아웃·가드

    # --- Web (DOM) 타깃 크레이트 ---
    lace-web-dom/             # 모든 DOM/웹 기능 빌더 + Leptos식 템플릿 복제·수술적 패칭 + `persistent_signal`·오프라인 큐·a11y
    lace-web-ssr/             # 스트리밍 HTML SSR·정적 사이트 생성(SSG)·`<Title>`/`<Meta>` Head 수집 (`ring` 서빙 연동용)
    lace-web-ionic/           # Ionic 레퍼런스 기반 적응형 웹 UI 컴포넌트·제스처·트랜지션
    lace-web-dev/             # 최소 CRA 수준의 로컬 개발용 프리뷰·번들·HMR 데브 서버 (`hyper preview` 연동)

    # --- Native (GPU + 필수 CPU 폴백) 타깃 크레이트 ---
    lace-native-gpu/          # GPUI/Rune식 GPU 배칭 렌더 백엔드 (쿼드·경로·그림자·글리프·ExternalTexture 제로카피 합성)
    lace-native-cpu/          # 필수 CPU 소프트웨어 폴백 렌더 백엔드 (동일 ScenePrimitive 소비)
    lace-native-text/         # 네이티브 폰트 탐색, 텍스트 셰이핑, 줄바꿈, 서브픽셀/SDF 글리프 아틀라스
    lace-native-sys/          # 크로스플랫폼 네이티브 SDK (파일 다이얼로그·드래그앤드롭·클립보드·공유 시트·Keychain·알림·딥링크·AccessKit 접근성)
    lace-platform-desktop/    # macOS, Windows, Linux 멀티 윈도우·네이티브 메뉴바·시스템 트레이·단축키·VSync·IME 어댑터
    lace-platform-ios/        # iOS UIKit/Metal·CADisplayLink·Safe Area·키보드 회피·멀티터치·햅틱·수명주기 어댑터
    lace-platform-android/    # Android NativeWindow·Choreographer·Vulkan/GLES·Safe Area·키보드 회피·백 제스처·햅틱 어댑터

    # --- 공용 통신 및 옵트인 웹뷰 어댑터 ---
    lace-adapter-client/      # 웹/네이티브 공용 Dual-Rail(`hyper-serde`+WebTransport / HTTP+WS) 및 인라인 서버 함수 클라이언트
    lace-adapter-webview/     # [옵트인 전용] 네이티브 창 일부에 `lace-web-dom` 출력물을 웹뷰 서피스로 띄울 때만 사용
```

## 2. 계층 의존 방향과 `hyper` 코어(`ref`) 수직화

```
  [ Web 타깃: lace-web-{dom,ssr,ionic,dev} ]    [ Native 타깃: lace-native-{gpu,cpu,text} + lace-platform-* ]
                       \                                              /
                        ▼                                            ▼
                   [ lace-core / lace-layout / lace-router / lace-macros ]
                                             │
                                             ▼ (코어 ref)
                       [ hyper (core / state / parser / serde / api) ]
```

- **단일 반응성 및 UI 문법 (`hyper-state`, `hyper-parser`)**: 웹 타깃과 네이티브 타깃이 모두 `hyper-state`와 `lace-core` 뷰 트리를 공유하므로, 컴포넌트와 비즈니스 상태 로직을 두 번 작성하지 않습니다.
- **네이티브 GPU & CPU 백엔드의 대칭성**: 네이티브 타깃에서는 `lace-core`가 플랫폼 중립 `ScenePrimitive` 리스트를 생성하고, `lace-native-gpu`와 `lace-native-cpu`(필수 폴백)가 동일한 프리미티브를 소비해 렌더링합니다.
- **네이티브 WebView 격리 보장**: `lace-native-*`와 `lace-platform-*` 기본 트리에는 어떠한 WebView 의존성도 링크되지 않습니다. 웹뷰가 꼭 필요한 응용에서만 같은 레포 내의 옵트인 어댑터(`lace-adapter-webview`)를 켜서 `lace-web-dom` 출력물을 웹뷰 서피스로 마운트합니다.
- **내부 포크 및 `unit optimization`**: 웹 DOM 패칭 및 네이티브 GPU/CPU·레이아웃·텍스트 포크 크레이트의 최적화 절차는 `hyperish/docs`의 `05-development-rules.md` §7을 따릅니다.
