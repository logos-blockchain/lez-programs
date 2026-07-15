import QtQuick
import QtQuick.Controls.Basic

Button {
    id: root

    property url iconSource
    property string accessibleName: ""
    property color iconColor: "#d4d4d8"

    implicitWidth: 36
    implicitHeight: 36
    display: AbstractButton.IconOnly
    hoverEnabled: true

    Accessible.name: root.accessibleName
    ToolTip.text: root.accessibleName
    ToolTip.visible: root.hovered && root.accessibleName.length > 0

    icon.source: root.iconSource
    icon.width: 18
    icon.height: 18
    icon.color: root.iconColor

    background: Rectangle {
        color: root.pressed ? "#3f3f46" : root.hovered || root.activeFocus ? "#27272a" : "transparent"
        radius: 6
        border.width: root.activeFocus ? 1 : 0
        border.color: "#f26a21"
    }
}
