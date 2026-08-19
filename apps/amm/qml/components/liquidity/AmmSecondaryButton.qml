import QtQuick
import QtQuick.Controls

// Outline counterpart to AmmPrimaryButton, for the second action in a pair
// (e.g. "Add liquidity" next to "Swap") where only one should carry the CTA fill.
Button {
    id: root

    required property var theme

    activeFocusOnTab: true
    focusPolicy: Qt.StrongFocus
    hoverEnabled: true
    implicitHeight: 44

    Accessible.name: text

    contentItem: Text {
        text: root.text
        color: root.enabled ? root.theme.colors.textPrimary : root.theme.colors.textPlaceholder
        font.pixelSize: 15
        font.weight: Font.Medium
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
        elide: Text.ElideRight
    }

    background: Rectangle {
        radius: 20
        color: !root.enabled
               ? "transparent"
               : root.pressed
                 ? root.theme.colors.panelHoverBg
                 : root.hovered || root.activeFocus
                   ? root.theme.colors.panelBg
                   : "transparent"
        border.color: root.enabled
                      ? (root.activeFocus ? root.theme.colors.textPrimary
                                          : root.theme.colors.borderStrong)
                      : root.theme.colors.border
        border.width: 1

        Behavior on color {
            ColorAnimation { duration: 120 }
        }
    }
}
