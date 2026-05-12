import QtQuick 2.15
import QtQuick.Layouts 1.15

Item {
    id: root

    property string label: ""
    property string value: ""
    property bool estimated: false
    property string estimateHelp: qsTr("This value is derived from your LP token balance, total LP supply, and current pool reserves.")

    implicitHeight: Math.max(18, Math.max(labelText.implicitHeight, valueGroup.implicitHeight))

    RowLayout {
        anchors.fill: parent
        spacing: 8

        Text {
            id: labelText

            color: "#A9A098"
            elide: Text.ElideRight
            font.pixelSize: 12
            text: root.label
            verticalAlignment: Text.AlignVCenter

            Layout.fillWidth: true
        }

        RowLayout {
            id: valueGroup

            spacing: 4

            Layout.alignment: Qt.AlignRight | Qt.AlignVCenter

            Text {
                color: "#E7E1D8"
                elide: Text.ElideRight
                font.bold: true
                font.pixelSize: 12
                horizontalAlignment: Text.AlignRight
                text: root.value
                verticalAlignment: Text.AlignVCenter

                Layout.maximumWidth: Math.max(178, root.width * 0.55)
            }

            EstimateInfoButton {
                enabled: root.estimated
                helpText: root.estimateHelp
                opacity: root.estimated ? 1 : 0
                visible: root.estimated

                Layout.preferredHeight: 18
                Layout.preferredWidth: root.estimated ? 18 : 0
            }
        }
    }
}
