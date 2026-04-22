import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Rectangle {
    id: root

    property int currentIndex: 0

    signal tabRequested(int index)

    color: "#1D1D1D"
    implicitHeight: 44
    radius: 8
    border.color: "#343434"
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
                color: root.currentIndex === 0 || addTab.hovered || addTab.activeFocus ? "#151515" : "#A9A098"
                elide: Text.ElideRight
                font.bold: true
                font.pixelSize: 12
                horizontalAlignment: Text.AlignHCenter
                text: addTab.text
                verticalAlignment: Text.AlignVCenter
            }

            background: Rectangle {
                border.color: addTab.activeFocus ? "#F26A21" : root.currentIndex === 0 ? "#F26A21" : "#151515"
                border.width: 1
                color: addTab.pressed ? "#D95C1E" : root.currentIndex === 0 ? "#F26A21" : addTab.hovered || addTab.activeFocus ? "#E7E1D8" : "#151515"
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
                color: root.currentIndex === 1 || removeTab.hovered || removeTab.activeFocus ? "#151515" : "#A9A098"
                elide: Text.ElideRight
                font.bold: true
                font.pixelSize: 12
                horizontalAlignment: Text.AlignHCenter
                text: removeTab.text
                verticalAlignment: Text.AlignVCenter
            }

            background: Rectangle {
                border.color: removeTab.activeFocus ? "#F26A21" : root.currentIndex === 1 ? "#F26A21" : "#151515"
                border.width: 1
                color: removeTab.pressed ? "#D95C1E" : root.currentIndex === 1 ? "#F26A21" : removeTab.hovered || removeTab.activeFocus ? "#E7E1D8" : "#151515"
                radius: 6
            }
        }
    }
}
