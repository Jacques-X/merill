# Merill Native Design System

Merill is an editorial Malta news reader. The interface should make reporting, source perspective, and reading comfort feel primary. Liquid Glass is a functional layer for navigation and controls, not a decorative treatment for article content.

## Apple References

- [Adopting Liquid Glass](https://developer.apple.com/documentation/technologyoverviews/adopting-liquid-glass)
- [SwiftUI Liquid Glass APIs](https://developer.apple.com/documentation/swiftui/glass)
- [`glassEffect(_:in:)`](https://developer.apple.com/documentation/swiftui/view/glasseffect(_:in:))
- [`tabBarMinimizeBehavior(_:)`](https://developer.apple.com/documentation/swiftui/view/tabbarminimizebehavior(_:))
- [`ToolbarSpacer`](https://developer.apple.com/documentation/swiftui/toolbarspacer)
- [Human Interface Guidelines: Materials](https://developer.apple.com/design/human-interface-guidelines/materials)

## Principles

1. Prefer native SwiftUI controls. Their system materials, contrast behavior, motion, and accessibility adaptation are the design system.
2. Keep the content layer quiet. Story imagery, headlines, summaries, timelines, source cards, forms, and article text use standard surfaces without decorative glass.
3. Reserve custom glass for exceptional transient controls. The feed filter expansion is the approved custom use because its morph communicates state and preserves reading context.
4. Use native navigation hierarchy. Root tabs, toolbars, menus, sheets, toggles, pickers, alerts, disclosure groups, and swipe actions should remain familiar.
5. Let controls recede. On supported iPhone systems, the tab bar minimizes while reading downward and returns when navigation is needed.

## Compatibility

- iOS and iPadOS 26 plus macOS 26 use system Liquid Glass where the framework supplies it.
- iOS and iPadOS 16 through 25 plus macOS 13 through 15 use semantic materials with the same information architecture.
- Respect system appearance, Reduce Transparency, Increase Contrast, Reduce Motion, Dynamic Type, VoiceOver, and keyboard navigation.

## Approved Custom Glass

- `FeedFilterControl` may use `GlassEffectContainer`, `glassEffect`, and `glassEffectID` on supported platforms for a compact filter-to-options transition.
- Do not add glass story cards, glass article bodies, decorative blur backgrounds, tinted glass on inactive controls, or custom optical effects around native bars.
