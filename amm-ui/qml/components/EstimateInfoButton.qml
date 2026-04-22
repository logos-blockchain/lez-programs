import QtQuick 2.15
import QtQuick.Controls 2.15

Button {
    id: root

    property string helpText: qsTr("This value is derived from your LP token balance, total LP supply, and current pool reserves.")

    activeFocusOnTab: true
    focusPolicy: Qt.StrongFocus
    hoverEnabled: true
    text: qsTr("?")

    Accessible.name: qsTr("Why this value is an estimate")

    onClicked: estimatePopup.opened ? estimatePopup.close() : estimatePopup.open()

    Keys.onEscapePressed: estimatePopup.close()

    contentItem: Text {
        color: root.activeFocus || root.hovered || estimatePopup.opened ? "#F26A21" : "#E7E1D8"
        font.bold: true
        font.pixelSize: 11
        horizontalAlignment: Text.AlignHCenter
        text: qsTr("i")
        verticalAlignment: Text.AlignVCenter
    }

    background: Rectangle {
        border.color: root.activeFocus || estimatePopup.opened ? "#F26A21" : "#343434"
        border.width: 1
        color: root.pressed ? "#2A221D" : "#1D1D1D"
        radius: 8
    }

    Popup {
        id: estimatePopup

        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        focus: true
        modal: false
        padding: 10
        width: 224
        x: Math.max(-width + root.width, -196)
        y: root.height + 4

        onClosed: root.forceActiveFocus()

        background: Rectangle {
            border.color: "#343434"
            border.width: 1
            color: "#1D1D1D"
            radius: 8
        }

        contentItem: Text {
            color: "#E7E1D8"
            font.pixelSize: 12
            lineHeight: 1.25
            text: root.helpText
            wrapMode: Text.WordWrap
        }
    }
}
