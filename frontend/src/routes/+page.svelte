  <script lang="ts">
    import { onDestroy } from 'svelte';

    const BACKEND_URL = 'http://127.0.0.1:3000';

    type EventType =
        | 'Plan'
        | 'ToolCall'
        | 'Diff'
        | 'Test'
        | 'Verify'
        | 'Completed'
        | 'Error';

    type AgentEvent = {
        type: EventType;
        data: {
            message?: string;
            tool?: string;
            input?: string;
            file?: string;
            content?: string;
            command?: string;
            output?: string;
            success?: boolean;
        };
    };

    let prompt = $state('');
    let taskId = $state('');
    let events = $state<AgentEvent[]>([]);
    let loading = $state(false);
    let error = $state('');
    let connected = $state(false);
    let approved = $state(false);
    let activeTab = $state<'overview' | 'diff' | 'activity'>('overview');
    let elapsed = $state(0);
    let currentStep = $state('Ready');
    let eventSource: EventSource | null = null;
    let timer: ReturnType<typeof setInterval> | null = null;

    // Dynamic greeting and date — existing agent logic remains unchanged.
    let currentDate = $state(new Date());

    function getGreeting(date: Date) {
        const hour = date.getHours();

        if (hour < 12) return 'Good Morning';
        if (hour < 17) return 'Good Afternoon';
        if (hour < 21) return 'Good Evening';
        return 'Good Night';
    }

    function getFormattedDate(date: Date) {
        return new Intl.DateTimeFormat('en-IN', {
            weekday: 'long',
            day: 'numeric',
            month: 'long',
            year: 'numeric'
        }).format(date);
    }

    const workflow = [
        'Plan',
        'Context',
        'Gemini',
        'Tools',
        'Diff',
        'Verify'
    ];

    function startTimer() {
        stopTimer();

        elapsed = 0;

        timer = setInterval(() => {
            elapsed += 1;
        }, 1000);
    }

    function stopTimer() {
        if (timer) {
            clearInterval(timer);
            timer = null;
        }
    }

    function formatTime(seconds: number) {
        const minutes = Math.floor(seconds / 60);
        const secs = seconds % 60;

        return `${String(minutes).padStart(2, '0')}:${String(secs).padStart(2, '0')}`;
    }

    function updateCurrentStep(event: AgentEvent) {
        if (event.type === 'Plan') {
            const message =
                event.data.message?.toLowerCase() ?? '';

            if (
                message.includes('workspace') ||
                message.includes('file') ||
                message.includes('context')
            ) {
                currentStep = 'Context';
            } else if (
                message.includes('gemini') ||
                message.includes('planning')
            ) {
                currentStep = 'Gemini';
            } else {
                currentStep = 'Plan';
            }
        }

        if (event.type === 'ToolCall') {
            currentStep = 'Tools';
        }

        if (event.type === 'Diff') {
            currentStep = 'Diff';
        }

        if (event.type === 'Test') {
            currentStep = 'Verify';
        }

        if (event.type === 'Verify') {
            currentStep = 'Verify';
        }

        if (event.type === 'Completed') {
            currentStep = 'Completed';
            stopTimer();
        }

        if (event.type === 'Error') {
            currentStep = 'Error';
            stopTimer();
        }
    }

    function addEvent(event: AgentEvent) {
        events = [...events, event];
        updateCurrentStep(event);
    }

    async function startAgent() {
        if (!prompt.trim()) {
            error = 'Enter a coding task first.';
            return;
        }

        closeStream();

        loading = true;
        error = '';
        approved = false;
        events = [];
        taskId = '';
        connected = false;
        currentStep = 'Starting';
        startTimer();

        try {
            const response = await fetch(
                `${BACKEND_URL}/tasks`,
                {
                    method: 'POST',
                    headers: {
                        'Content-Type': 'application/json'
                    },
                    body: JSON.stringify({
                        prompt: prompt.trim()
                    })
                }
            );

            if (!response.ok) {
                throw new Error(
                    `Task creation failed: ${response.status}`
                );
            }

            const task = await response.json();

            taskId = task.id ?? task.task_id;

            if (!taskId) {
                throw new Error(
                    'Backend did not return a task ID.'
                );
            }

            connectStream(taskId);
        } catch (err) {
            loading = false;
            stopTimer();

            error =
                err instanceof Error
                    ? err.message
                    : 'Failed to start agent.';
        }
    }

    function connectStream(id: string) {
        eventSource = new EventSource(
            `${BACKEND_URL}/tasks/${id}/stream`
        );

        eventSource.onopen = () => {
            connected = true;
        };

        eventSource.onmessage = (event) => {
            try {
                const agentEvent =
                    JSON.parse(event.data) as AgentEvent;

                addEvent(agentEvent);

                if (agentEvent.type === 'Completed') {
                    loading = false;
                }

                if (agentEvent.type === 'Error') {
                    loading = false;

                    error =
                        agentEvent.data.message ??
                        'Agent execution failed.';
                }
            } catch {
                console.error(
                    'Invalid SSE event:',
                    event.data
                );
            }
        };

        eventSource.onerror = () => {
            connected = false;

            if (loading) {
                /*
                 * EventSource automatically attempts
                 * reconnection.
                 */
                return;
            }

            closeStream();
        };
    }

    function closeStream() {
        if (eventSource) {
            eventSource.close();
            eventSource = null;
        }

        connected = false;
    }

    function stopAgent() {
        loading = false;
        stopTimer();
        closeStream();
        currentStep = 'Stopped';
    }

    async function approveTask() {
        if (!taskId) {
            error =
                'No task is available for approval.';
            return;
        }

        try {
            error = '';

            const response = await fetch(
                `${BACKEND_URL}/tasks/${taskId}/approve`,
                {
                    method: 'POST'
                }
            );

            if (!response.ok) {
                throw new Error(
                    `Approval failed: ${response.status}`
                );
            }

            const result = await response.json();

            approved =
                result.approved === true;

            if (approved) {
                currentStep = 'Completed';
            }
        } catch (err) {
            error =
                err instanceof Error
                    ? err.message
                    : 'Failed to approve task.';
        }
    }

    function getEventTitle(event: AgentEvent) {
        switch (event.type) {
            case 'Plan':
                return 'Planning';

            case 'ToolCall':
                return 'Tool Call';

            case 'Diff':
                return 'File Change';

            case 'Test':
                return 'Test';

            case 'Verify':
                return 'Verification';

            case 'Completed':
                return 'Completed';

            case 'Error':
                return 'Error';

            default:
                return 'Agent Event';
        }
    }

    function getEventMessage(event: AgentEvent) {
        switch (event.type) {
            case 'Plan':
                return event.data.message ?? '';

            case 'ToolCall':
                return `${event.data.tool ?? 'tool'} → ${event.data.input ?? ''}`;

            case 'Diff':
                return event.data.file ?? '';

            case 'Test':
                return event.data.command ?? '';

            case 'Verify':
                return event.data.message ?? '';

            case 'Completed':
                return event.data.message ?? '';

            case 'Error':
                return event.data.message ?? '';

            default:
                return '';
        }
    }

    function getEventIcon(type: EventType) {
        switch (type) {
            case 'Plan':
                return '✦';

            case 'ToolCall':
                return '⚙';

            case 'Diff':
                return '◇';

            case 'Test':
                return '▷';

            case 'Verify':
                return '✓';

            case 'Completed':
                return '✓';

            case 'Error':
                return '!';
        }
    }

    function isStepActive(step: string) {
        const currentIndex =
            workflow.indexOf(currentStep);

        const stepIndex =
            workflow.indexOf(step);

        return (
            currentIndex >= stepIndex &&
            currentIndex >= 0
        );
    }

    function extractFiles() {
        return events
            .filter(
                (event) =>
                    event.type === 'Diff' &&
                    event.data.file
            )
            .map(
                (event) =>
                    event.data.file as string
            );
    }

    function diffEvents() {
        return events.filter(
            (event) =>
                event.type === 'Diff'
        );
    }

    function toolEvents() {
        return events.filter(
            (event) =>
                event.type === 'ToolCall'
        );
    }

    const dateTimer = setInterval(() => {
        currentDate = new Date();
    }, 60000);

    onDestroy(() => {
        closeStream();
        stopTimer();
        clearInterval(dateTimer);
    });
</script>

<svelte:head>
    <title>AI Coding Agent</title>
    <meta
        name="description"
        content="AI-powered autonomous coding agent"
    />
</svelte:head>

<div class="app">

    <!-- SIDEBAR -->

    <aside class="sidebar">

        <div class="brand">
            <div class="brand-icon">
                AI
            </div>

            <div>
                <strong>AI Coding Agent</strong>
                <span>Autonomous Developer</span>
            </div>
        </div>

        <div class="sidebar-section">

            <div class="section-label">
                AGENT
            </div>

            <button
                class:active={activeTab === 'overview'}
                onclick={() =>
                    (activeTab = 'overview')}
            >
                <span>◈</span>
                Overview
            </button>

            <button
                class:active={activeTab === 'diff'}
                onclick={() =>
                    (activeTab = 'diff')}
            >
                <span>◇</span>
                Changes
                {#if diffEvents().length}
                    <b>{diffEvents().length}</b>
                {/if}
            </button>

            <button
                class:active={activeTab === 'activity'}
                onclick={() =>
                    (activeTab = 'activity')}
            >
                <span>≡</span>
                Activity
                {#if events.length}
                    <b>{events.length}</b>
                {/if}
            </button>
        </div>

        <div class="sidebar-section">

            <div class="section-label">
                WORKFLOW
            </div>

            {#each workflow as step, index}
                <div class:step-active={isStepActive(step)} class="workflow-item">
                    <span class="workflow-number">
                        {index + 1}
                    </span>

                    <span>{step}</span>

                    {#if isStepActive(step)}
                        <span class="workflow-check">
                            ✓
                        </span>
                    {/if}
                </div>
            {/each}
        </div>

        <div class="sidebar-bottom">

            <div class="system-card">

                <div class="system-title">
                    System
                </div>

                <div class="system-row">
                    <span>Backend</span>

                    <span class:offline={!connected}>
                        <i></i>
                        {connected
                            ? 'Connected'
                            : 'Offline'}
                    </span>
                </div>

                <div class="system-row">
                    <span>Model</span>
                    <span>Gemini</span>
                </div>

                <div class="system-row">
                    <span>Transport</span>
                    <span>SSE</span>
                </div>

            </div>

        </div>

    </aside>


    <!-- MAIN -->

    <main class="main">

        <!-- TOP BAR -->

        <header class="topbar">

            <div>
                <div class="eyebrow">
                    AI DEVELOPMENT ENVIRONMENT
                </div>

                <h1>
                    Coding Agent
                </h1>
            </div>

            <div class="top-actions">

                <div class="connection">

                    <span
                        class:offline={!connected}
                        class="connection-dot"
                    ></span>

                    {connected
                        ? 'Agent connected'
                        : 'Waiting for agent'}
                </div>

                <div class="timer">
                    ⏱ {formatTime(elapsed)}
                </div>

            </div>

        </header>


        <!-- HERO -->

        <section class="hero">

            <div class="welcome-card">
                <div class="welcome-icon">✦</div>
                <div class="welcome-info">
                    <div class="welcome-greeting">
                        {getGreeting(currentDate)}
                    </div>
                    <div class="welcome-date">
                        {getFormattedDate(currentDate)}
                    </div>
                </div>
                <div class="welcome-time">
                    {currentDate.toLocaleTimeString('en-IN', {
                        hour: '2-digit',
                        minute: '2-digit'
                    })}
                </div>
            </div>

            <div class="hero-content">

                <div class="status-badge">
                    <span></span>

                    {loading
                        ? currentStep
                        : 'Ready'}
                </div>

                <h2>
                    Build software
                    <span>with AI.</span>
                </h2>

                <p>
                    Describe what you want to build.
                    The agent plans, edits, tests and
                    verifies the implementation.
                </p>

            </div>

        </section>


        <!-- TASK INPUT -->

        <section class="task-card">

            <div class="task-header">

                <div>
                    <div class="card-kicker">
                        NEW TASK
                    </div>

                    <h3>
                        What should the agent build?
                    </h3>
                </div>

                {#if taskId}
                    <div class="task-id">
                        TASK
                        <code>{taskId.slice(0, 8)}</code>
                    </div>
                {/if}

            </div>

            <textarea
                bind:value={prompt}
                placeholder="Example: Create a login page with email and password validation..."
                disabled={loading}
            ></textarea>

            <div class="task-footer">

                <div class="hint">
                    <span>↵</span>
                    Agent will analyze the workspace
                    before making changes.
                </div>

                <div class="buttons">

                    {#if loading}

                        <button
                            class="secondary"
                            onclick={stopAgent}
                        >
                            Stop Agent
                        </button>

                    {:else}

                        <button
                            class="primary"
                            onclick={startAgent}
                        >
                            <span>✦</span>
                            Start Agent
                        </button>

                    {/if}

                </div>

            </div>

        </section>


        <!-- ERROR -->

        {#if error}

            <div class="error-box">
                <div class="error-icon">!</div>

                <div>
                    <strong>Agent error</strong>
                    <p>{error}</p>
                </div>
            </div>

        {/if}


        <!-- DASHBOARD -->

        {#if taskId}

            <section class="dashboard">

                <!-- PROGRESS -->

                <div class="progress-card">

                    <div class="progress-top">

                        <div>
                            <span class="card-kicker">
                                EXECUTION
                            </span>

                            <h3>
                                Agent progress
                            </h3>
                        </div>

                        <strong>
                            {events.length} events
                        </strong>

                    </div>

                    <div class="progress-line">

                        {#each workflow as step, index}

                            <div
                                class:complete={isStepActive(step)}
                                class:current={
                                    currentStep === step
                                }
                                class="progress-step"
                            >
                                <div class="progress-dot">
                                    {#if isStepActive(step)}
                                        ✓
                                    {:else}
                                        {index + 1}
                                    {/if}
                                </div>

                                <span>{step}</span>
                            </div>

                        {/each}

                    </div>

                </div>


                <!-- TABS -->

                <div class="tabs">

                    <button
                        class:tab-active={
                            activeTab === 'overview'
                        }
                        onclick={() =>
                            (activeTab = 'overview')}
                    >
                        Overview
                    </button>

                    <button
                        class:tab-active={
                            activeTab === 'diff'
                        }
                        onclick={() =>
                            (activeTab = 'diff')}
                    >
                        Changes
                        <span>
                            {diffEvents().length}
                        </span>
                    </button>

                    <button
                        class:tab-active={
                            activeTab === 'activity'
                        }
                        onclick={() =>
                            (activeTab = 'activity')}
                    >
                        Live Activity
                    </button>

                </div>


                <!-- OVERVIEW -->

                {#if activeTab === 'overview'}

                    <div class="grid">

                        <!-- LIVE TIMELINE -->

                        <div class="panel timeline-panel">

                            <div class="panel-header">

                                <div>
                                    <span class="card-kicker">
                                        LIVE STREAM
                                    </span>

                                    <h3>
                                        Agent Activity
                                    </h3>
                                </div>

                                <div class="live">
                                    <span></span>
                                    LIVE
                                </div>

                            </div>

                            <div class="timeline">

                                {#if events.length === 0}

                                    <div class="empty">
                                        Waiting for agent
                                        events...
                                    </div>

                                {:else}

                                    {#each events as event, index}

                                        <div class="event">

                                            <div class="event-marker">

                                                <div class:event-error={
                                                    event.type === 'Error'
                                                } class:event-success={
                                                    event.type === 'Completed' ||
                                                    event.type === 'Verify'
                                                }>
                                                    {getEventIcon(
                                                        event.type
                                                    )}
                                                </div>

                                                {#if index < events.length - 1}
                                                    <span></span>
                                                {/if}

                                            </div>

                                            <div class="event-content">

                                                <div class="event-top">

                                                    <strong>
                                                        {getEventTitle(
                                                            event
                                                        )}
                                                    </strong>

                                                    <small>
                                                        #{index + 1}
                                                    </small>

                                                </div>

                                                <p>
                                                    {getEventMessage(
                                                        event
                                                    )}
                                                </p>

                                                {#if event.type === 'Diff'}

                                                    <pre>{event.data.content ?? ''}</pre>

                                                {/if}

                                            </div>

                                        </div>

                                    {/each}

                                {/if}

                            </div>

                        </div>


                        <!-- RIGHT COLUMN -->

                        <div class="right-column">

                            <!-- FILES -->

                            <div class="panel">

                                <div class="panel-header">

                                    <div>
                                        <span class="card-kicker">
                                            WORKSPACE
                                        </span>

                                        <h3>
                                            Changed Files
                                        </h3>
                                    </div>

                                    <span class="count">
                                        {extractFiles().length}
                                    </span>

                                </div>

                                <div class="file-list">

                                    {#if extractFiles().length === 0}

                                        <div class="empty-small">
                                            No changes yet
                                        </div>

                                    {:else}

                                        {#each extractFiles() as file}
                                            <div class="file">

                                                <span class="file-icon">
                                                    ◇
                                                </span>

                                                <span>
                                                    {file}
                                                </span>

                                                <span class="modified">
                                                    modified
                                                </span>

                                            </div>
                                        {/each}

                                    {/if}

                                </div>

                            </div>


                            <!-- TOOLS -->

                            <div class="panel">

                                <div class="panel-header">

                                    <div>
                                        <span class="card-kicker">
                                            TOOL LAYER
                                        </span>

                                        <h3>
                                            Tool Calls
                                        </h3>
                                    </div>

                                    <span class="count">
                                        {toolEvents().length}
                                    </span>

                                </div>

                                <div class="tool-list">

                                    {#if toolEvents().length === 0}

                                        <div class="empty-small">
                                            No tool calls yet
                                        </div>

                                    {:else}

                                        {#each toolEvents() as event}
                                            <div class="tool">

                                                <span>
                                                    ⚙
                                                </span>

                                                <div>
                                                    <strong>
                                                        {event.data.tool}
                                                    </strong>

                                                    <small>
                                                        {event.data.input}
                                                    </small>
                                                </div>

                                            </div>
                                        {/each}

                                    {/if}

                                </div>

                            </div>

                        </div>

                    </div>


                    <!-- APPROVAL -->

                    {#if !approved && currentStep !== 'Completed'}

                        <section class="approval">

                            <div class="approval-icon">
                                🔐
                            </div>

                            <div class="approval-content">

                                <span class="card-kicker">
                                    DEVELOPER CONTROL
                                </span>

                                <h3>
                                    Review and approve changes
                                </h3>

                                <p>
                                    The agent has prepared its
                                    proposed changes. Nothing
                                    should be applied until you
                                    approve the task.
                                </p>

                            </div>

                            <button
                                class="approve"
                                onclick={approveTask}
                            >
                                Approve Changes
                                <span>→</span>
                            </button>

                        </section>

                    {:else if approved}

                        <section class="approved">

                            <div class="approved-icon">
                                ✓
                            </div>

                            <div>
                                <strong>
                                    Changes approved
                                </strong>

                                <p>
                                    The agent was authorized to
                                    apply the proposed changes.
                                </p>
                            </div>

                        </section>

                    {/if}

                {:else if activeTab === 'diff'}

                    <div class="panel diff-panel">

                        <div class="panel-header">

                            <div>
                                <span class="card-kicker">
                                    PROPOSED CHANGES
                                </span>

                                <h3>
                                    Code Diff
                                </h3>
                            </div>

                        </div>

                        {#if diffEvents().length === 0}

                            <div class="empty">
                                No file changes have been
                                proposed yet.
                            </div>

                        {:else}

                            {#each diffEvents() as event}

                                <div class="diff-file">

                                    <div class="diff-file-header">
                                        <span>
                                            ◇
                                        </span>

                                        <strong>
                                            {event.data.file}
                                        </strong>
                                    </div>

                                    <pre>{event.data.content ?? ''}</pre>

                                </div>

                            {/each}

                        {/if}

                    </div>

                {:else}

                    <div class="panel activity-panel">

                        <div class="panel-header">

                            <div>
                                <span class="card-kicker">
                                    EVENT STREAM
                                </span>

                                <h3>
                                    Raw Agent Activity
                                </h3>
                            </div>

                        </div>

                        <div class="activity-list">

                            {#each events as event}

                                <div class="activity-row">

                                    <span class="activity-type">
                                        {event.type}
                                    </span>

                                    <code>
                                        {JSON.stringify(
                                            event.data
                                        )}
                                    </code>

                                </div>

                            {/each}

                        </div>

                    </div>

                {/if}

            </section>

        {/if}

    </main>

</div>


<style>
    :global(*) {
        box-sizing: border-box;
    }

    :global(body) {
        margin: 0;
        background: #f6f7fb;
        color: #182033;
        font-family:
            Inter,
            ui-sans-serif,
            system-ui,
            -apple-system,
            BlinkMacSystemFont,
            "Segoe UI",
            sans-serif;
    }

    :global(button),
    :global(textarea) {
        font: inherit;
    }

    .app {
        min-height: 100vh;
        display: flex;
    }

    /* SIDEBAR */

    .sidebar {
        width: 250px;
        min-height: 100vh;
        background: #111827;
        color: #d7dce7;
        padding: 24px 16px;
        display: flex;
        flex-direction: column;
        position: fixed;
        left: 0;
        top: 0;
        bottom: 0;
    }

    .brand {
        display: flex;
        gap: 12px;
        align-items: center;
        padding: 4px 8px 30px;
    }

    .brand-icon {
        width: 38px;
        height: 38px;
        border-radius: 11px;
        display: grid;
        place-items: center;
        background: #6d5dfc;
        color: white;
        font-size: 12px;
        font-weight: 800;
    }

    .brand strong {
        display: block;
        color: white;
        font-size: 14px;
    }

    .brand span {
        display: block;
        color: #7f899d;
        font-size: 11px;
        margin-top: 3px;
    }

    .sidebar-section {
        margin-bottom: 28px;
    }

    .section-label {
        padding: 0 10px;
        margin-bottom: 9px;
        font-size: 9px;
        letter-spacing: 1.5px;
        font-weight: 800;
        color: #697388;
    }

    .sidebar button {
        width: 100%;
        border: 0;
        background: transparent;
        color: #9da6b8;
        padding: 10px;
        border-radius: 8px;
        display: flex;
        align-items: center;
        gap: 11px;
        text-align: left;
        cursor: pointer;
        margin-bottom: 3px;
    }

    .sidebar button:hover,
    .sidebar button.active {
        background: #20283a;
        color: white;
    }

    .sidebar button b {
        margin-left: auto;
        font-size: 10px;
        background: #313b51;
        padding: 2px 6px;
        border-radius: 10px;
    }

    .workflow-item {
        padding: 9px 10px;
        display: flex;
        align-items: center;
        gap: 10px;
        font-size: 12px;
        color: #677286;
    }

    .workflow-item.step-active {
        color: #e8ebf2;
    }

    .workflow-number {
        width: 21px;
        height: 21px;
        display: grid;
        place-items: center;
        border: 1px solid #3a4355;
        border-radius: 50%;
        font-size: 9px;
    }

    .step-active .workflow-number {
        background: #6d5dfc;
        border-color: #6d5dfc;
        color: white;
    }

    .workflow-check {
        margin-left: auto;
        color: #7dffbc;
        font-size: 11px;
    }

    .sidebar-bottom {
        margin-top: auto;
    }

    .system-card {
        background: #181f2e;
        border: 1px solid #283247;
        border-radius: 10px;
        padding: 13px;
    }

    .system-title {
        color: white;
        font-size: 11px;
        font-weight: 700;
        margin-bottom: 12px;
    }

    .system-row {
        display: flex;
        justify-content: space-between;
        gap: 10px;
        font-size: 10px;
        color: #697388;
        margin-top: 8px;
    }

    .system-row span:last-child {
        color: #aab3c2;
    }

    .system-row span:last-child:not(.offline) {
        color: #78e7a9;
    }

    .system-row i {
        width: 6px;
        height: 6px;
        border-radius: 50%;
        display: inline-block;
        background: #67e89a;
        margin-right: 4px;
    }

    .system-row .offline i {
        background: #778094;
    }

    /* MAIN */

    .main {
        margin-left: 250px;
        width: calc(100% - 250px);
        min-height: 100vh;
        padding: 0 38px 60px;
    }

    .topbar {
        min-height: 82px;
        display: flex;
        align-items: center;
        justify-content: space-between;
        border-bottom: 1px solid #e5e8ef;
    }

    .eyebrow,
    .card-kicker {
        font-size: 9px;
        letter-spacing: 1.5px;
        font-weight: 800;
        color: #7d879a;
    }

    .topbar h1 {
        margin: 3px 0 0;
        font-size: 20px;
        letter-spacing: -0.5px;
    }

    .top-actions {
        display: flex;
        gap: 10px;
        align-items: center;
    }

    .connection,
    .timer {
        border: 1px solid #e1e5ec;
        background: white;
        padding: 8px 11px;
        border-radius: 8px;
        font-size: 10px;
        color: #687286;
    }

    .connection-dot {
        width: 7px;
        height: 7px;
        background: #3ddc84;
        border-radius: 50%;
        display: inline-block;
        margin-right: 5px;
    }

    .connection-dot.offline {
        background: #9ca3af;
    }

    /* DYNAMIC WELCOME */

    .welcome-card {
        display: flex;
        align-items: center;
        gap: 13px;
        width: fit-content;
        margin-bottom: 24px;
        padding: 10px 13px 10px 10px;
        border: 1px solid #e3e1fb;
        border-radius: 14px;
        background:
            linear-gradient(135deg, #ffffff 0%, #f8f7ff 100%);
        box-shadow: 0 8px 24px rgba(75, 65, 170, 0.07);
        animation: welcomeIn 0.55s ease-out;
    }

    .welcome-icon {
        width: 38px;
        height: 38px;
        display: grid;
        place-items: center;
        border-radius: 11px;
        color: white;
        background: linear-gradient(135deg, #6d5dfc, #887cff);
        box-shadow: 0 7px 16px rgba(109, 93, 252, 0.22);
        font-size: 16px;
        animation: welcomePulse 2.5s ease-in-out infinite;
    }

    .welcome-info {
        min-width: 190px;
    }

    .welcome-greeting {
        color: #20263a;
        font-size: 15px;
        font-weight: 800;
        letter-spacing: -0.2px;
    }

    .welcome-date {
        margin-top: 3px;
        color: #858da0;
        font-size: 10px;
    }

    .welcome-time {
        padding-left: 12px;
        border-left: 1px solid #e5e3f3;
        color: #6254d8;
        font-size: 11px;
        font-weight: 800;
        white-space: nowrap;
    }

    @keyframes welcomeIn {
        from {
            opacity: 0;
            transform: translateY(-7px);
        }
        to {
            opacity: 1;
            transform: translateY(0);
        }
    }

    @keyframes welcomePulse {
        0%, 100% {
            transform: scale(1);
        }
        50% {
            transform: scale(1.06);
        }
    }

    /* HERO */

    .hero {
        padding: 44px 0 30px;
    }

    .hero-content {
        max-width: 680px;
    }

    .status-badge {
        display: inline-flex;
        align-items: center;
        gap: 7px;
        border: 1px solid #dedcf9;
        background: #f2f0ff;
        color: #6254d8;
        border-radius: 20px;
        padding: 6px 10px;
        font-size: 10px;
        font-weight: 700;
    }

    .status-badge span {
        width: 6px;
        height: 6px;
        border-radius: 50%;
        background: #6d5dfc;
    }

    .hero h2 {
        margin: 16px 0 10px;
        font-size: clamp(36px, 5vw, 56px);
        line-height: 1;
        letter-spacing: -3px;
    }

    .hero h2 span {
        color: #6d5dfc;
    }

    .hero p {
        margin: 0;
        color: #778196;
        font-size: 14px;
        line-height: 1.7;
        max-width: 560px;
    }

    /* TASK */

    .task-card,
    .panel,
    .progress-card {
        background: white;
        border: 1px solid #e2e6ee;
        border-radius: 13px;
        box-shadow: 0 4px 18px rgba(25, 35, 55, 0.035);
    }

    .task-card {
        padding: 20px;
    }

    .task-header {
        display: flex;
        justify-content: space-between;
        gap: 20px;
        align-items: center;
        margin-bottom: 15px;
    }

    .task-header h3,
    .panel h3,
    .progress-card h3 {
        margin: 4px 0 0;
        font-size: 14px;
    }

    .task-id {
        color: #8a93a3;
        font-size: 9px;
        letter-spacing: 1px;
    }

    .task-id code {
        color: #565f70;
        margin-left: 5px;
    }

    textarea {
        width: 100%;
        min-height: 110px;
        resize: vertical;
        border: 1px solid #dfe3eb;
        border-radius: 9px;
        padding: 14px;
        outline: none;
        color: #242b39;
        background: #fafbfc;
        line-height: 1.6;
        font-size: 13px;
    }

    textarea:focus {
        border-color: #9d94ff;
        box-shadow: 0 0 0 3px rgba(109, 93, 252, 0.08);
        background: white;
    }

    .task-footer {
        display: flex;
        justify-content: space-between;
        align-items: center;
        gap: 15px;
        margin-top: 13px;
    }

    .hint {
        color: #8a93a3;
        font-size: 10px;
    }

    .hint span {
        background: #eef0f4;
        padding: 2px 5px;
        border-radius: 4px;
        margin-right: 4px;
    }

    button {
        cursor: pointer;
    }

    .primary,
    .approve {
        border: 0;
        color: white;
        background: #6254df;
        border-radius: 8px;
        padding: 10px 16px;
        font-weight: 700;
        font-size: 11px;
        box-shadow: 0 5px 15px rgba(98, 84, 223, 0.18);
    }

    .primary:hover,
    .approve:hover {
        background: #5144ca;
    }

    .secondary {
        border: 1px solid #dfe3eb;
        background: white;
        border-radius: 8px;
        padding: 9px 14px;
        font-size: 11px;
        color: #596273;
    }

    /* ERROR */

    .error-box {
        margin-top: 15px;
        display: flex;
        gap: 12px;
        padding: 14px;
        border-radius: 10px;
        border: 1px solid #ffd6d6;
        background: #fff6f6;
        color: #c83c3c;
    }

    .error-icon {
        width: 24px;
        height: 24px;
        border-radius: 50%;
        background: #ffe1e1;
        display: grid;
        place-items: center;
        font-weight: 800;
    }

    .error-box strong {
        font-size: 12px;
    }

    .error-box p {
        margin: 3px 0 0;
        font-size: 11px;
    }

    /* DASHBOARD */

    .dashboard {
        margin-top: 22px;
    }

    .progress-card {
        padding: 18px 20px 20px;
    }

    .progress-top {
        display: flex;
        justify-content: space-between;
        align-items: center;
    }

    .progress-top > strong {
        font-size: 10px;
        color: #7f899a;
    }

    .progress-line {
        display: grid;
        grid-template-columns: repeat(6, 1fr);
        margin-top: 22px;
    }

    .progress-step {
        position: relative;
        color: #a0a7b5;
        font-size: 9px;
    }

    .progress-step:not(:last-child)::after {
        content: "";
        position: absolute;
        top: 10px;
        left: 25px;
        right: 0;
        height: 1px;
        background: #e1e5ec;
    }

    .progress-step.complete:not(:last-child)::after {
        background: #8176ed;
    }

    .progress-dot {
        position: relative;
        z-index: 2;
        width: 20px;
        height: 20px;
        border-radius: 50%;
        background: #f1f3f6;
        border: 1px solid #e0e4eb;
        display: grid;
        place-items: center;
        margin-bottom: 7px;
        font-size: 8px;
    }

    .progress-step.complete {
        color: #5f54ca;
    }

    .progress-step.complete .progress-dot {
        background: #6d5dfc;
        border-color: #6d5dfc;
        color: white;
    }

    .progress-step.current .progress-dot {
        box-shadow: 0 0 0 5px rgba(109, 93, 252, 0.1);
    }

    /* TABS */

    .tabs {
        display: flex;
        gap: 5px;
        border-bottom: 1px solid #e2e6ee;
        margin-top: 25px;
    }

    .tabs button {
        border: 0;
        background: transparent;
        padding: 11px 13px;
        color: #7d8797;
        font-size: 11px;
        border-bottom: 2px solid transparent;
    }

    .tabs button.tab-active {
        color: #5145c6;
        border-bottom-color: #6d5dfc;
        font-weight: 700;
    }

    .tabs span {
        background: #eef0f5;
        border-radius: 10px;
        padding: 2px 5px;
        margin-left: 4px;
        font-size: 8px;
    }

    /* GRID */

    .grid {
        display: grid;
        grid-template-columns: minmax(0, 1.55fr) minmax(280px, 0.75fr);
        gap: 16px;
        margin-top: 16px;
    }

    .panel {
        overflow: hidden;
    }

    .panel-header {
        display: flex;
        justify-content: space-between;
        align-items: center;
        padding: 17px 18px;
        border-bottom: 1px solid #edf0f4;
    }

    .live {
        display: flex;
        align-items: center;
        gap: 5px;
        color: #40a76b;
        font-size: 8px;
        font-weight: 800;
        letter-spacing: 1px;
    }

    .live span {
        width: 5px;
        height: 5px;
        background: #4cd98a;
        border-radius: 50%;
    }

    .count {
        background: #f0f2f6;
        color: #7b8494;
        padding: 4px 7px;
        border-radius: 8px;
        font-size: 9px;
    }

    /* TIMELINE */

    .timeline {
        padding: 18px;
        max-height: 550px;
        overflow-y: auto;
    }

    .event {
        display: flex;
        gap: 12px;
    }

    .event-marker {
        width: 25px;
        flex: 0 0 25px;
        display: flex;
        flex-direction: column;
        align-items: center;
    }

    .event-marker > div {
        width: 25px;
        height: 25px;
        display: grid;
        place-items: center;
        border-radius: 7px;
        background: #eeecff;
        color: #6356d8;
        font-size: 10px;
        font-weight: 800;
    }

    .event-marker > div.event-success {
        background: #e9faf1;
        color: #32a868;
    }

    .event-marker > div.event-error {
        background: #fff0f0;
        color: #d64b4b;
    }

    .event-marker > span {
        width: 1px;
        flex: 1;
        min-height: 24px;
        background: #e5e8ed;
    }

    .event-content {
        padding-bottom: 20px;
        min-width: 0;
        flex: 1;
    }

    .event-top {
        display: flex;
        justify-content: space-between;
        gap: 10px;
    }

    .event-top strong {
        font-size: 11px;
    }

    .event-top small {
        color: #a1a8b5;
        font-size: 8px;
    }

    .event-content p {
        margin: 5px 0 0;
        color: #697487;
        font-size: 11px;
        line-height: 1.55;
        white-space: pre-wrap;
        overflow-wrap: anywhere;
    }

    pre {
        margin: 9px 0 0;
        background: #111827;
        color: #cfd6e2;
        border-radius: 7px;
        padding: 11px;
        font-family: "Cascadia Code", Consolas, monospace;
        font-size: 9px;
        line-height: 1.6;
        overflow: auto;
        white-space: pre-wrap;
    }

    /* RIGHT */

    .right-column {
        display: flex;
        flex-direction: column;
        gap: 16px;
    }

    .file-list,
    .tool-list {
        padding: 7px;
    }

    .file {
        display: flex;
        align-items: center;
        gap: 8px;
        padding: 9px;
        border-radius: 7px;
        font-size: 10px;
    }

    .file:hover {
        background: #f6f7fa;
    }

    .file-icon {
        color: #695ee1;
    }

    .file > span:nth-child(2) {
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        flex: 1;
    }

    .modified {
        color: #56aa79;
        font-size: 8px;
    }

    .tool {
        display: flex;
        gap: 9px;
        padding: 9px;
        border-radius: 7px;
    }

    .tool > span {
        color: #7569e7;
    }

    .tool strong {
        display: block;
        font-size: 10px;
    }

    .tool small {
        display: block;
        color: #8c95a5;
        margin-top: 3px;
        font-size: 9px;
        overflow-wrap: anywhere;
    }

    .empty,
    .empty-small {
        color: #9aa2b0;
        text-align: center;
    }

    .empty {
        padding: 45px 20px;
        font-size: 11px;
    }

    .empty-small {
        padding: 18px 8px;
        font-size: 10px;
    }

    /* APPROVAL */

    .approval,
    .approved {
        margin-top: 16px;
        border-radius: 12px;
        padding: 18px;
        display: flex;
        align-items: center;
        gap: 15px;
    }

    .approval {
        background: #fffdf5;
        border: 1px solid #eee4b8;
    }

    .approved {
        background: #f1fcf5;
        border: 1px solid #c8ecd7;
    }

    .approval-icon,
    .approved-icon {
        width: 40px;
        height: 40px;
        border-radius: 10px;
        display: grid;
        place-items: center;
        flex: 0 0 40px;
    }

    .approval-icon {
        background: #f6edc6;
    }

    .approved-icon {
        background: #d9f4e3;
        color: #2e9e60;
        font-weight: 800;
    }

    .approval-content {
        flex: 1;
    }

    .approval-content h3 {
        margin: 4px 0;
        font-size: 13px;
    }

    .approval-content p,
    .approved p {
        margin: 0;
        color: #7f827c;
        font-size: 10px;
        line-height: 1.5;
    }

    .approve {
        white-space: nowrap;
    }

    /* DIFF */

    .diff-panel,
    .activity-panel {
        margin-top: 16px;
    }

    .diff-file {
        border-bottom: 1px solid #e8ebef;
    }

    .diff-file:last-child {
        border-bottom: 0;
    }

    .diff-file-header {
        padding: 12px 18px;
        display: flex;
        gap: 8px;
        align-items: center;
        background: #fafbfc;
        font-size: 10px;
    }

    .diff-file-header span {
        color: #685de0;
    }

    .diff-file pre {
        margin: 0;
        border-radius: 0;
        max-height: 400px;
    }

    /* ACTIVITY */

    .activity-list {
        padding: 10px;
        max-height: 600px;
        overflow: auto;
    }

    .activity-row {
        display: flex;
        gap: 12px;
        padding: 9px;
        border-bottom: 1px solid #f0f1f4;
        align-items: flex-start;
    }

    .activity-type {
        width: 70px;
        flex: 0 0 70px;
        color: #665bd4;
        font-size: 9px;
        font-weight: 800;
    }

    .activity-row code {
        font-size: 9px;
        color: #6d7686;
        overflow-wrap: anywhere;
        white-space: pre-wrap;
    }

    /* RESPONSIVE */

    @media (max-width: 1000px) {
        .sidebar {
            width: 210px;
        }

        .main {
            margin-left: 210px;
            width: calc(100% - 210px);
            padding-left: 24px;
            padding-right: 24px;
        }

        .grid {
            grid-template-columns: 1fr;
        }
    }

    @media (max-width: 720px) {
        .sidebar {
            display: none;
        }

        .main {
            margin-left: 0;
            width: 100%;
            padding: 0 15px 40px;
        }

        .topbar {
            min-height: 70px;
        }

        .connection {
            display: none;
        }

        .hero h2 {
            letter-spacing: -2px;
        }

        .welcome-card {
            width: 100%;
        }

        .welcome-info {
            min-width: 0;
            flex: 1;
        }

        .welcome-time {
            padding-left: 8px;
        }

        .task-footer,
        .approval {
            align-items: stretch;
            flex-direction: column;
        }

        .buttons,
        .primary,
        .approve {
            width: 100%;
        }

        .progress-line {
            overflow-x: auto;
            min-width: 600px;
        }

        .progress-card {
            overflow-x: auto;
        }
    }
</style>