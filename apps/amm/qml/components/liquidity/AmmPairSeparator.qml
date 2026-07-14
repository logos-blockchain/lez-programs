import QtQuick
import QtQuick.Controls

Item {
    id: root

    required property var theme
    property string symbol: "\u2193"
    property string accessibleName: qsTr("Swap token order")

    signal clicked

    implicitHeight: 40

    Rectangle {
        anchors.verticalCenter: parent.verticalCenter
        anchors.left: parent.left
        anchors.right: parent.right
        height: 1
        color: root.theme.colors.divider
    }

    Button {
        id: button

        anchors.centerIn: parent
        width: 36
        height: 36
        hoverEnabled: true
        enabled: root.enabled

        Accessible.name: root.accessibleName
        ToolTip.visible: hovered
        ToolTip.text: Accessible.name

        onClicked: root.clicked()

        contentItem: Text {
            text: root.symbol
            color: button.enabled
                   ? root.theme.colors.textPrimary
                   : root.theme.colors.textPlaceholder
            font.pixelSize: 16
            horizontalAlignment: Text.AlignHCenter
            verticalAlignment: Text.AlignVCenter
        }

        background: Rectangle {
            radius: 18
            color: button.hovered || button.activeFocus
                   ? root.theme.colors.panelHoverBg
                   : root.theme.colors.panelBg
            border.color: root.theme.colors.borderStrong
            border.width: 1

            Behavior on color {
                ColorAnimation { duration: 120 }
            }
        }
    }
}
