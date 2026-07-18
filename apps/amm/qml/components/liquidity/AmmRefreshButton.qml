import QtQuick
import QtQuick.Controls

Button {
    id: root

    required property var theme
    property string accessibleName: qsTr("Refresh position data")

    implicitWidth: 40
    implicitHeight: 40
    hoverEnabled: true
    text: "\u21bb"

    Accessible.name: root.accessibleName
    ToolTip.visible: hovered
    ToolTip.text: Accessible.name

    contentItem: Text {
        text: root.text
        color: root.enabled ? root.theme.colors.textSecondary
                            : root.theme.colors.textPlaceholder
        font.pixelSize: 20
        font.weight: Font.Medium
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
    }

    background: Rectangle {
        radius: width / 2
        color: root.pressed ? root.theme.colors.selection
                            : root.hovered || root.activeFocus
                              ? root.theme.colors.panelHoverBg
                              : root.theme.colors.panelBg
        border.color: root.theme.colors.borderStrong
        border.width: 1
    }
}
