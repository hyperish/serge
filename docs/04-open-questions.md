# 04. 열린 결정 (lace)

> 기준일: 2026-11-17

`lace` 레포 범위의 미정 사항 목록입니다. 범용 명세 전체에 걸친 미정 사항은 [`hyperish/docs/docs/07-open-questions.md`](https://github.com/hyperish/docs/blob/main/docs/07-open-questions.md)에서 추적합니다.

## 열려 있는 결정

| # | 결정 항목 | 왜 중요한가 | 지금의 방향(제안) |
|---|---|---|---|
| 1 | 포크·기반 대상 크레이트 선정 (`rsx!` 파서 및 DOM 반응성 렌더러) | Dioxus `rsx!` DX와 Leptos의 타키온(tachys)/세밀한 DOM 파이프라인을 어떻게 결합·포크할지 결정 필요 | 파서/매크로 프론트엔드는 Dioxus `rsx` 계열, 뷰 렌더링 백엔드는 Leptos `tachys` 계열을 레퍼런스/포크 후보로 두고 `hyper-state` 위에서 통합 |
| 2 | `lace-ionic`의 스타일링·토큰 엔진 | 적응형 웹 컴포넌트의 스타일 캡슐화와 테마 토큰 방식 결정 필요 | CSS 커스텀 프로퍼티 기반 토큰 + 컴파일 타임 스코프 스타일 매크로 검토 |
| 3 | `lace-adapter-webview`와 `hera` 간 브리지 프로토콜 | 웹뷰 어댑터 사용 시 네이티브(`hera`)와 웹(`lace`) 간 상태·이벤트 동기화 방식 | `hyper-serde` 기반 타입 안전 양방향 IPC 채널 및 `hyper-state` 미러링 어댑터 제안 |
| 4 | WASM 번들 크기 및 초기 로딩 최적화 기준 | 웹 환경에서 첫 화면 도달 시간(DX 원칙 1)에 직결됨 | 코드 스플리팅·아일랜드 Hydration 범위 및 WASM 크기 예산 설정 |

## 결정됨

- **결정**: `lace`는 DOM/웹 전반을 다루며, DX는 Dioxus의 `rsx!`·매크로, 렌더링·반응성 파이프라인은 Leptos, 크로스플랫폼 웹 UI 및 웹뷰 호환은 Ionic을 레퍼런스로 삼고 `hyper` 코어를 `ref`로 수직 통합함
  - **날짜**: 2026-11-17
  - **영향**: `lace` 전체 문서(`00`~`03`)
