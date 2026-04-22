import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Rectangle {
    id: root

    property int currentIndex: 0

    signal tabRequested(int index)

    color: "#181818"
    implicitHeight: 42
    radius: 8
    border.color: "#303030"
    border.width: 1

    RowLayout {
        anchors.fill: parent
        anchors.margins: 4
        spacing: 4

        Button {
            id: addTab

            activeFocusOnTab: true
            focusPolicy: Qt.StrongFocus
            hoverEnabled: true
            text: qsTr("Add liquidity")

            Accessible.name: addTab.text
            Accessible.role: Accessible.PageTab

            Layout.fillHeight: true
            Layout.fillWidth: true

            onClicked: root.tabRequested(0)

            contentItem: Text {
                color: root.currentIndex === 0 ? "#F2D8C7" : addTab.hovered || addTab.activeFocus ? "#E7E1D8" : "#8E8780"
                elide: Text.ElideRight
                font.bold: true
                font.pixelSize: 12
                horizontalAlignment: Text.AlignHCenter
                text: addTab.text
                verticalAlignment: Text.AlignVCenter
            }

            background: Rectangle {
                border.color: addTab.activeFocus || root.currentIndex === 0 ? "#F26A21" : "#181818"
                border.width: 1
                color: addTab.pressed ? "#2A1D16" : root.currentIndex === 0 ? "#211914" : addTab.hovered || addTab.activeFocus ? "#202020" : "#121212"
                radius: 6
            }
        }

        Button {
            id: removeTab

            activeFocusOnTab: true
            focusPolicy: Qt.StrongFocus
            hoverEnabled: true
            text: qsTr("Remove liquidity")

            Accessible.name: removeTab.text
            Accessible.role: Accessible.PageTab

            Layout.fillHeight: true
            Layout.fillWidth: true

            onClicked: root.tabRequested(1)

            contentItem: Text {
                color: root.currentIndex === 1 ? "#F2D8C7" : removeTab.hovered || removeTab.activeFocus ? "#E7E1D8" : "#8E8780"
                elide: Text.ElideRight
                font.bold: true
                font.pixelSize: 12
                horizontalAlignment: Text.AlignHCenter
                text: removeTab.text
                verticalAlignment: Text.AlignVCenter
            }

            background: Rectangle {
                border.color: removeTab.activeFocus || root.currentIndex === 1 ? "#F26A21" : "#181818"
                border.width: 1
                color: removeTab.pressed ? "#2A1D16" : root.currentIndex === 1 ? "#211914" : removeTab.hovered || removeTab.activeFocus ? "#202020" : "#121212"
                radius: 6
            }
        }
    }
}
