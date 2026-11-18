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

- **웹(DOM)과 네이티브(GPU+CPU 폴백) UI를 `lace` 단일 레포로 통합** (2026-11-18): `00-overview.md`
- **UI 뷰 마크업은 proc macro DSL(QML/Slint + Dioxus `rsx!` 레퍼런스)로 고정하고, 도메인/라우트/가드 조립은 Rust 체이닝으로 고정** (2026-11-18): `01-dx-and-dsl.md`
- **Web 타깃(DOM 출력 + Leptos 파이프라인 + Ionic형 UI + CRA급 로컬 데브 서버) 및 `ring` 프로덕션 서빙 분리 확정** (2026-11-17): `00-overview.md`, `03-adapters-and-pipeline.md`
- **Native 타깃(GPUI·`anomalyco/rune` 기반 GPU + 필수 CPU 폴백, 데스크톱·iOS·Android 지원, 네이티브 UI 내 WebView 절대 금지 및 옵트인 `lace-adapter-webview` 분리) 확정** (2026-11-17 ~ 2026-11-18): `00-overview.md`, `02-architecture.md`
