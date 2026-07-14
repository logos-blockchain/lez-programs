import QtQml

QtObject {
    property bool isDark: true
    readonly property var colors: isDark ? dark : light

    readonly property var light: ({
        "background": "#F4EDE3",
        "cardBg": "#FFFFFF",
        "inputBg": "#EFE7DB",
        "panelBg": "#E7E1D8",
        "panelHoverBg": "#D9D0C2",
        "textPrimary": "#151515",
        "textSecondary": "#7D756E",
        "textPlaceholder": "#A9A098",
        "border": Qt.rgba(0, 0, 0, 0.08),
        "borderStrong": Qt.rgba(0, 0, 0, 0.10),
        "divider": Qt.rgba(0, 0, 0, 0.06),
        "ctaBg": "#F26A21",
        "ctaHoverBg": "#D95C1E",
        "ctaPressedBg": "#C85018",
        "selection": "#F2D8C7",
        "noTokenCircle": "#A9A098",
        "success": "#4F9B64",
        "warning": "#B8732A",
        "error": "#D85F4B"
    })

    readonly property var dark: ({
        "background": "#151515",
        "cardBg": "#1B1B1B",
        "inputBg": "#101010",
        "panelBg": "#181818",
        "panelHoverBg": "#202020",
        "textPrimary": "#E7E1D8",
        "textSecondary": "#A9A098",
        "textPlaceholder": "#8E8780",
        "border": Qt.rgba(1, 1, 1, 0.08),
        "borderStrong": Qt.rgba(1, 1, 1, 0.10),
        "divider": Qt.rgba(1, 1, 1, 0.06),
        "ctaBg": "#F26A21",
        "ctaHoverBg": "#FF8A3D",
        "ctaPressedBg": "#D95C1E",
        "selection": "#211914",
        "noTokenCircle": "#343434",
        "success": "#78C88D",
        "warning": "#F2B366",
        "error": "#F08A76"
    })
}
