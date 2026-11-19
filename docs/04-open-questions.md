# 04. 열린 결정 (serge)

> 기준일: 2026-11-19

`serge` 레포 범위의 미정 사항 목록입니다. 범용 명세 전체의 결정 기록은 [`hyperish/docs/docs/07-open-questions.md`](https://github.com/hyperish/docs/blob/main/docs/07-open-questions.md)에서 추적합니다.

## 열려 있는 결정

| # | 결정 항목 | 왜 중요한가 | 지금의 방향(제안) |
|---|---|---|---|
| 1 | 외부 3D/그래픽 엔진(`opale` 등) 뷰포트와 `serge` 네이티브/웹 UI의 GPU 텍스처 공유 세부 규격 | 리얼리스틱 그래픽 엔진 에디터에서 3D 뷰포트 위에 `serge` UI를 제로카피로 합성해야 함 | Native에서는 공유 GPU 디바이스 또는 네이티브 텍스처 핸들(`IOSurface`, `DXGI`, `AHardwareBuffer`, `dma-buf`)을 `ScenePrimitive::ExternalTexture`로 합성하고, Web에서는 WebGPU 캔버스 슬롯으로 마운트 |
| 2 | GPU → CPU 폴백 전환 정책 (Native 타깃) | 런타임에 어떤 조건에서 CPU 폴백(`serge-native-cpu`)으로 자동 전환할지 결정 필요 | GPU 디바이스 생성 실패 시 자동 폴백 + 환경 변수(`HYPERISH_RENDER_BACKEND=cpu`) 및 헤드리스 테스트 모드 명시적 지정 지원 |
| 3 | `SergeBundleManifest`·`SergeSsrHandler` 시그니처 및 옵트인 `serge-adapter-webview` 동기화 규격 | `orbic` 프로덕션 서빙 핸드오프와 네이티브 내 옵트인 웹뷰 서피스의 상태·이벤트 동기화(`seq`, 리로드 복구) 세부 고정 필요 | `hyve-serde`(`rkyv`) 기반 순서 보장 봉투(`id`, `seq`)와 버전 태그가 포함된 최소 매니페스트·트레이트로 확정 |

## 결정됨

- **웹(DOM)과 네이티브(GPU+CPU 폴백) UI를 `serge` 단일 레포로 통합** (2026-11-18): `00-overview.md`
- **UI 뷰 마크업은 `ui!` proc macro DSL 5대 최소 문법으로 고정하고, 기능·라우트·가드 조립은 `serge::feature()` Rust 체이닝으로 고정** (2026-11-18 ~ 2026-11-19): `01-dx-and-dsl.md` (`orbic::component()`·`#[component]` 심볼 충돌 해소 및 `web::*`/`native::*` 네임스페이스 확정 포함)
- **Web 타깃(DOM 출력 + Leptos 파이프라인 + Ionic형 UI + CRA급 로컬 데브 서버) 및 `orbic` 프로덕션 서빙 분리 확정** (2026-11-17): `00-overview.md`, `03-adapters-and-pipeline.md`
- **Native 타깃(GPUI·`anomalyco/rune` 레퍼런스 기반 GPU + 필수 CPU 폴백, 데스크톱·iOS·Android 지원, 네이티브 UI 내 WebView 절대 금지 및 옵트인 `serge-adapter-webview` 분리) 확정** (2026-11-17 ~ 2026-11-18): `00-overview.md`, `02-architecture.md`
- **공용 스타일 IR(`serge-style`)과 네이티브 전용 레이아웃 솔버(`serge-native-layout`) 분리 및 iOS Dynamic Island·위젯의 정적 `ui!` → SwiftUI/Glance 코드젠 원칙 확정** (2026-11-19): `00-overview.md`, `02-architecture.md`, `03-adapters-and-pipeline.md`
- **crates.io 비충돌 고유명 `serge`(구 `lace`) 및 순수 `MIT` 라이선스 확정** (2026-11-19): 레포명·crates.io 패키지명·Rust 크레이트명을 `serge`로 1:1 일치
- **MIT 포크 스택(`tachys`·`taffy`·`wgpu`·`vello`·`vello_cpu`·`parley`+`swash`·`accesskit`, `winit` 외부 의존성 전용), 내부 코어 개조 방향 및 Phase 1~3 우선순위 컷 확정** (2026-11-19):
  - Phase 1(`v0.1.0`): `serge-core`·`serge-macros`(`ui!` 5대 문법)·`serge-style`·`serge-router` + Web DOM(`tachys`·`serge-web-dev`) + Desktop Native(`taffy`·`wgpu`·`vello_cpu` 폴백·`parley`/`swash`·`winit`)
  - Phase 2(`v0.2.0`): `serge-web-ssr`(SSR/SSG/SEO)·`serge-web-ionic`·`persistent_signal`·Desktop 멀티 윈도우/메뉴바/트레이/`AccessKit`/Shm IPC
  - Phase 3(`v0.3.0+`): iOS/Android 풀 HW·SW SDK + Dynamic Island/WidgetKit/Glance 코드젠 + 옵트인 `serge-adapter-webview` (`02-architecture.md` §3)
