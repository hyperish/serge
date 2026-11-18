# 04. 열린 결정 (lace)

> 기준일: 2026-11-17

`lace` 레포 범위의 미정 사항 목록입니다. 범용 명세 전체에 걸친 미정 사항은 [`hyperish/docs/docs/07-open-questions.md`](https://github.com/hyperish/docs/blob/main/docs/07-open-questions.md)에서 추적합니다.

## 열려 있는 결정

| # | 결정 항목 | 왜 중요한가 | 지금의 방향(제안) |
|---|---|---|---|
| 1 | 포크·기반 대상 크레이트 선정 (DOM 반응성 렌더러 및 데브 번들러) | Leptos의 `tachys`/세밀한 DOM 파이프라인과 CRA급 데브 서버(`lace-dev`)를 구현할 포크 대상 확정 필요 | 뷰 렌더링 백엔드는 Leptos `tachys` 계열을 레퍼런스/포크 후보로 두고 `hyper-state` 위에서 1단계 `unit optimization`(추상화 제거·코어 직결) 수행 |
| 2 | `lace-ionic`의 스타일링·토큰 엔진 | 적응형 웹 컴포넌트의 스타일 캡슐화와 테마 토큰 방식 결정 필요 | CSS 커스텀 프로퍼티 기반 토큰 + 컴파일 타임 스코프 스타일 매크로/빌더 검토 |
| 3 | WASM 번들 크기 및 초기 로딩 최적화 기준 | 웹 환경에서 첫 화면 도달 시간(DX 원칙 1)에 직결됨 | 코드 스플리팅·아일랜드 Hydration 범위 및 WASM 크기 예산 설정 |
| 4 | `LaceBundleManifest` 및 `LaceSsrHandler`의 구체 필드·트레이트 시그니처 | `ring-adapter-lace` 및 `glint-adapter-lace-webview`가 `lace` 산출물을 받는 핸드오프 계약([`03-adapters-and-pipeline.md`](03-adapters-and-pipeline.md) §3)의 코드 수준 고정 필요 | 첫 통합 프로토타입 시 `hyper-api`/`hyper-serde` 버전 태그를 포함한 최소 매니페스트·스트림 트레이트로 확정 |

## 결정됨

- **결정**: `lace`의 역할은 **DOM/웹 기능을 전부 `hyper`/`lace`로 표현해 쉽게 구축하고 DOM으로 나오게 하는 것**이며, 최소 **CRA 수준의 로컬 프리뷰/데브 서빙**을 자체 내장함. 프로덕션 서빙은 `ring`, 네이티브 웹뷰 호스팅은 `glint`가 담당함
  - **날짜**: 2026-11-17
  - **영향**: `lace` 전체 문서(`00`~`03`), `ring`, `glint`
- **결정**: `glint`와 **동일한 UI 문법**(QML 기반 Slint 스타일 + Dioxus `rsx!`)을 공유하되, GPUI처럼 DSL 없이도 가독성이 좋으면 순수 Rust 빌더로 작성 가능하며, 파이프라인은 Leptos, 웹 UI는 Ionic을 레퍼런스로 삼음
  - **날짜**: 2026-11-17
  - **영향**: `lace` 전체 문서(`00`~`03`)
