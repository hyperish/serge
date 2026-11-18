# 04. 열린 결정 (lace)

> 기준일: 2026-11-18

`lace` 레포 범위의 미정 사항 목록입니다. 범용 명세 전체에 걸친 미정 사항은 [`hyperish/docs/docs/07-open-questions.md`](https://github.com/hyperish/docs/blob/main/docs/07-open-questions.md)에서 추적합니다.

## 열려 있는 결정

| # | 결정 항목 | 왜 중요한가 | 지금의 방향(제안) |
|---|---|---|---|
| 1 | 포크·기반 대상 크레이트 선정 (Web DOM 파이프라인 & Native GPU/CPU·레이아웃·텍스트) | 웹(Leptos형 DOM)과 네이티브(GPUI/`anomalyco/rune`형 GPU+CPU) 양쪽의 하위 포크 대상을 확정해 1단계 `unit optimization`을 시작해야 함 | 공용 레이아웃(`taffy`), 웹 DOM(`tachys` 계열), 네이티브 GPU(`wgpu`/`blade` 계열), 네이티브 CPU 폴백(`tiny-skia`/`vello_cpu` 계열), 텍스트(`cosmic-text`/`parley`/`swash`) 중 라이선스와 코어 직결성 기준으로 선정 |
| 2 | 공통 UI 프리미티브와 웹/네이티브 전용 확장의 네임스페이스 구분 | 하나의 UI 문법 안에서 공통 위젯과 웹 전용(DOM 태그)·네이티브 전용(OS 창/GPU 서피스) 기능을 어떻게 구분할지 고정 필요 | 기본 위젯(`View`, `Stack`, `Text`, `Button`, `Canvas` 등)은 양쪽 공통으로 두고, 타깃 전용 속성은 `web::*` / `native::*` 모듈 또는 타깃 어트리뷰트로 컴파일 타임 검증 |
| 3 | 외부 3D/그래픽 엔진 뷰포트와 `lace` 네이티브/웹 UI의 GPU 텍스처 공유 | 리얼리스틱 그래픽 엔진 에디터에서 3D 뷰포트 위에 `lace` UI를 제로카피로 합성해야 함 | Native에서는 공유 GPU 디바이스 또는 네이티브 텍스처 핸들(`IOSurface`, `DXGI`, `AHardwareBuffer`, `dma-buf`)을 `ScenePrimitive::ExternalTexture`로 합성하고, Web에서는 WebGPU 캔버스 슬롯으로 마운트 |
| 4 | GPU → CPU 폴백 전환 정책 (Native 타깃) | 런타임에 어떤 조건에서 CPU 폴백(`lace-native-cpu`)으로 자동 전환할지 결정 필요 | GPU 디바이스 생성 실패 시 자동 폴백 + 환경 변수(`HYPERISH_RENDER_BACKEND=cpu`) 및 헤드리스 테스트 모드 명시적 지정 지원 |
| 5 | `LaceBundleManifest`·`LaceSsrHandler` 시그니처 및 옵트인 `lace-adapter-webview` 동기화 규격 | `ring` 프로덕션 서빙 핸드오프와 네이티브 내 옵트인 웹뷰 서피스의 상태·이벤트 동기화(`seq`, 리로드 복구) 세부 고정 필요 | `hyper-serde` 기반 순서 보장 봉투(`id`, `seq`)와 버전 태그가 포함된 최소 매니페스트·트레이트로 확정 |

## 결정됨

- **결정**: 기존 네이티브 UI 레포(`glint`, 구 `hera`)를 없애고 **`lace` 하나로 웹(DOM)과 네이티브(GPU+CPU 폴백, 데스크톱·iOS·Android) UI를 모두 통합**함
  - **날짜**: 2026-11-18
  - **이유**: React Native, Flutter, Dioxus처럼 웹과 네이티브가 이미 동일한 UI 문법과 반응형 상태(`hyper-state`)를 공유하므로 단일 UI 프레임워크 안에서 웹 타깃과 네이티브 타깃을 함께 제공하는 것이 수직 통합과 DX에 가장 자연스러움
  - **영향**: `lace` 전체 문서(`00`~`04`), `hyperish/docs`, `hyper`, `ring`
- **결정**: Web 타깃은 모든 DOM/웹 기능을 `hyper`/`lace`로 표현해 DOM으로 출력하고 최소 **CRA 수준의 로컬 프리뷰/데브 서빙(`lace-web-dev`)**을 내장하며(파이프라인: Leptos, 웹 UI: Ionic), 프로덕션 서빙은 `ring`이 담당함
  - **날짜**: 2026-11-17
  - **영향**: `00-overview.md`, `02-architecture.md`, `03-adapters-and-pipeline.md`
- **결정**: Native 타깃은 GPUI 및 `anomalyco/rune`을 레퍼런스로 하는 GPU(필수 CPU 폴백 포함) 기반 네이티브 UI로 데스크톱·iOS·Android를 지원하며, **네이티브 UI에 WebView는 절대 사용하지 않고** 웹뷰가 꼭 필요한 경우에만 내부 옵트인 어댑터(`lace-adapter-webview`)로 Web 타깃 출력을 마운트함
  - **날짜**: 2026-11-17 (2026-11-18 `lace`로 통합)
  - **영향**: `00-overview.md`, `02-architecture.md`, `03-adapters-and-pipeline.md`
- **결정**: 공통 UI 문법은 QML 기반 Slint 스타일 + Dioxus `rsx!`를 레퍼런스로 삼되, DSL 채택 이유는 오직 개발 생산성이므로 GPUI처럼 순수 Rust 빌더만으로도 가독성이 좋거나 더 효율적인 방법이 있다면 어떤 방법을 써도 무관함
  - **날짜**: 2026-11-17
  - **영향**: `01-dx-and-dsl.md`
