import QtQuick
import QtQuick.Controls

Button {
    id: root

    required property var theme

    activeFocusOnTab: true
    focusPolicy: Qt.StrongFocus
    hoverEnabled: true
    implicitHeight: 56

    Accessible.name: text

    contentItem: Text {
        text: root.text
        color: root.enabled ? "#FFFFFF" : root.theme.colors.textSecondary
        font.pixelSize: 17
        font.weight: Font.Medium
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
        elide: Text.ElideRight
    }

    background: Rectangle {
        radius: 20
        color: !root.enabled
               ? root.theme.colors.panelBg
               : root.pressed
                 ? root.theme.colors.ctaPressedBg
                 : root.hovered || root.activeFocus
                   ? root.theme.colors.ctaHoverBg
                   : root.theme.colors.ctaBg
        border.color: root.activeFocus && root.enabled
                      ? root.theme.colors.textPrimary : "transparent"
        border.width: 1

        Behavior on color {
            ColorAnimation { duration: 120 }
        }
    }
}
