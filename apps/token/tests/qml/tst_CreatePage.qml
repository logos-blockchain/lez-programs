import QtQuick
import QtTest

import "../../qml/pages"

Item {
    id: root

    width: 1200
    height: 900

    Component {
        id: backendComponent

        QtObject {
            property bool isWalletOpen: true
            property string syncStatus: "ready"
            property bool initialSync: false
            property int createCalls: 0

            function createFungible(_definitionId, _holdingId, _name, _supply, _authority) {
                ++createCalls
                return ({})
            }
        }
    }

    Component {
        id: runtimeComponent

        QtObject {
            property var successCallback: null
            property var failureCallback: null

            function watch(_result, success, failure) {
                successCallback = success
                failureCallback = failure
            }

            function finishSuccess() {
                const callback = successCallback
                successCallback = null
                failureCallback = null
                if (callback)
                    callback({ status: "ok", transactionId: "a".repeat(64) })
            }

            function finishFailure() {
                const callback = failureCallback
                successCallback = null
                failureCallback = null
                if (callback)
                    callback("temporary_failure")
            }
        }
    }

    Component {
        id: storeComponent

        QtObject {
            property var drafts: []

            function addDraft(draft) {
                drafts = drafts.concat([draft])
                return draft
            }
        }
    }

    Component {
        id: pageComponent

        CreatePage {
            width: root.width
            height: root.height
        }
    }

    TestCase {
        name: "CreatePage"
        when: windowShown

        function createFixture() {
            const backend = createTemporaryObject(backendComponent, root)
            const runtime = createTemporaryObject(runtimeComponent, root)
            const store = createTemporaryObject(storeComponent, root)
            const page = createTemporaryObject(pageComponent, root, {
                backend: backend,
                runtime: runtime,
                store: store
            })
            verify(page, "Component exists")

            page.setPattern("fixed")
            findChild(page, "tokenDefinitionTargetField").text = "a".repeat(64)
            findChild(page, "tokenHoldingTargetField").text = "b".repeat(64)
            page.step = 2

            const prepareButton = findChild(page, "tokenPrepareButton")
            verify(prepareButton, "Object exists")
            tryCompare(prepareButton, "enabled", true)
            return { backend, runtime, store, page, prepareButton }
        }

        function test_rapidActivationsCreateOneSubmission() {
            const fixture = createFixture()

            mouseClick(fixture.prepareButton)
            tryCompare(fixture.page, "submitting", true)
            compare(fixture.backend.createCalls, 1)

            mouseClick(fixture.prepareButton)
            tryCompare(fixture.prepareButton, "enabled", false)
            compare(fixture.backend.createCalls, 1)

            fixture.runtime.finishSuccess()
            tryCompare(fixture.page, "prepared", true)
            tryCompare(fixture.page, "canSubmit", false)
            compare(fixture.store.drafts.length, 1)

            fixture.page.prepareDefinition()
            compare(fixture.backend.createCalls, 1)
        }

        function test_navigationAndEditsKeepOneInFlightRequest() {
            const fixture = createFixture()
            const originalDefinitionId = findChild(fixture.page, "tokenDefinitionTargetField").text

            mouseClick(fixture.prepareButton)
            tryCompare(fixture.page, "submitting", true)
            fixture.page.visible = false
            fixture.page.visible = true
            tryCompare(fixture.page, "submitting", true)

            findChild(fixture.page, "tokenDefinitionTargetField").text = "c".repeat(64)
            mouseClick(fixture.prepareButton)
            compare(fixture.backend.createCalls, 1)

            fixture.runtime.finishSuccess()
            tryCompare(fixture.page, "prepared", true)
            compare(fixture.store.drafts.length, 1)
            compare(fixture.store.drafts[0].id, originalDefinitionId)
        }

        function test_failureClearsLockForRetry() {
            const fixture = createFixture()

            mouseClick(fixture.prepareButton)
            tryCompare(fixture.page, "submitting", true)
            fixture.runtime.finishFailure()
            tryCompare(fixture.page, "submitting", false)
            tryCompare(fixture.page, "canSubmit", true)
            compare(fixture.store.drafts.length, 0)

            mouseClick(fixture.prepareButton)
            tryCompare(fixture.page, "submitting", true)
            compare(fixture.backend.createCalls, 2)
            fixture.runtime.finishSuccess()
            tryCompare(fixture.page, "prepared", true)
            compare(fixture.store.drafts.length, 1)
        }

        function test_waitsForWalletSynchronizationBeforeSubmit() {
            const fixture = createFixture()
            fixture.backend.syncStatus = "syncing"
            fixture.backend.initialSync = true
            tryCompare(fixture.page, "walletReady", false)
            tryCompare(fixture.prepareButton, "enabled", false)
            compare(fixture.backend.createCalls, 0)

            fixture.backend.syncStatus = "ready"
            fixture.backend.initialSync = false
            tryCompare(fixture.page, "walletReady", true)
            tryCompare(fixture.prepareButton, "enabled", true)
            mouseClick(fixture.prepareButton)
            compare(fixture.backend.createCalls, 1)
            fixture.runtime.finishFailure()
        }
    }
}
