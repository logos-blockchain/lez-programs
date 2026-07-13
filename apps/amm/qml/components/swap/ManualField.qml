import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

// A single labeled text field, styled to match the rest of the swap UI's
// theme. Used by ManualSwapPanel for the manual token id / holding address /
// amount inputs.
ColumnLayout {
    id: root

    property var theme
    property string label: ""
    property string placeholder: ""
    property string text: ""
    // When true, non-digit characters are stripped as the user types (used
    // for the base-units amount field).
    property bool numericOnly: false

    signal edited(string newValue)

    spacing: 4

    Text {
        text: root.label
        color: theme.colors.textSecondary
        font.pixelSize: 12
    }

    Rectangle {
        Layout.fillWidth: true
        Layout.preferredHeight: 40
        radius: 10
        color: theme.colors.inputBg
        border.color: field.activeFocus ? theme.colors.ctaBg : theme.colors.border
        border.width: 1
        Behavior on border.color { ColorAnimation { duration: 120 } }

        TextField {
            id: field
            anchors.fill: parent
            anchors.leftMargin: 10
            anchors.rightMargin: 10
            verticalAlignment: TextInput.AlignVCenter
            color: theme.colors.textPrimary
            placeholderTextColor: theme.colors.textPlaceholder
            placeholderText: root.placeholder
            font.pixelSize: 13
            selectByMouse: true
            background: Item {}

            Binding {
                target: field
                property: "text"
                value: root.text
                when: !field.activeFocus
            }

            onTextEdited: {
                var v = text;
                if (root.numericOnly) {
                    var stripped = v.replace(/[^0-9]/g, "");
                    if (stripped !== v) {
                        field.text = stripped;
                        return; // onTextEdited fires again for the corrected text
                    }
                    v = stripped;
                }
                root.edited(v);
            }
        }
    }
}
