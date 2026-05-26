import QtQuick 2.15
import QtQuick.Layouts 1.15

Item {
    id: root

    property var theme
    property string swapModeText: ""
    property string feeText: ""
    property string priceImpactText: ""
    property real priceImpactPercent: 0
    property string slippageText: ""
    property string minReceivedText: ""

    readonly property color priceImpactColor: {
        if (root.priceImpactPercent > 5) return "#F08A76";
        if (root.priceImpactPercent > 1) return "#F2B366";
        return root.theme.colors.textPrimary;
    }

    implicitHeight: column.implicitHeight

    ColumnLayout {
        id: column

        anchors.fill: parent
        spacing: 8

        Item {
            implicitHeight: 18
            visible: root.swapModeText.length > 0

            Layout.fillWidth: true

            Text {
                anchors.left: parent.left
                anchors.verticalCenter: parent.verticalCenter
                color: root.theme.colors.textSecondary
                font.pixelSize: 12
                text: qsTr("Type of swap")
            }

            Text {
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                color: root.theme.colors.textPrimary
                font.bold: true
                font.pixelSize: 12
                text: root.swapModeText
            }
        }

        Item {
            implicitHeight: 18

            Layout.fillWidth: true

            Text {
                anchors.left: parent.left
                anchors.verticalCenter: parent.verticalCenter
                color: root.theme.colors.textSecondary
                font.pixelSize: 12
                text: qsTr("Fee")
            }

            Text {
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                color: root.theme.colors.textPrimary
                font.bold: true
                font.pixelSize: 12
                text: root.feeText
            }
        }

        Item {
            implicitHeight: 18

            Layout.fillWidth: true

            Text {
                anchors.left: parent.left
                anchors.verticalCenter: parent.verticalCenter
                color: root.theme.colors.textSecondary
                font.pixelSize: 12
                text: qsTr("Price impact")
            }

            Text {
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                color: root.priceImpactColor
                font.bold: true
                font.pixelSize: 12
                text: root.priceImpactText
            }
        }

        Item {
            implicitHeight: 18
            visible: root.slippageText.length > 0

            Layout.fillWidth: true

            Text {
                anchors.left: parent.left
                anchors.verticalCenter: parent.verticalCenter
                color: root.theme.colors.textSecondary
                font.pixelSize: 12
                text: qsTr("Slippage tolerance")
            }

            Text {
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                color: root.theme.colors.textPrimary
                font.bold: true
                font.pixelSize: 12
                text: root.slippageText
            }
        }

        Item {
            implicitHeight: 18

            Layout.fillWidth: true

            Text {
                anchors.left: parent.left
                anchors.verticalCenter: parent.verticalCenter
                color: root.theme.colors.textSecondary
                font.pixelSize: 12
                text: qsTr("Min received")
            }

            Text {
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                color: root.theme.colors.textPrimary
                font.bold: true
                font.pixelSize: 12
                text: root.minReceivedText
            }
        }
    }
}
