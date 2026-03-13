// Reference implementations for accessible media controls.
// Standards: TYPESCRIPT.md > Accessibility

function SeekSlider({ position, duration, onSeek }: SeekSliderProps) {
    return (
        <input
            type="range"
            aria-label="Seek position"
            aria-valuetext={`${formatTime(position)} of ${formatTime(duration)}`}
            min={0}
            max={duration}
            value={position}
            onChange={(e) => onSeek(Number(e.target.value))}
        />
    );
}

function VolumeSlider({ volume, onChange }: VolumeSliderProps) {
    const percentage = Math.round(volume * 100);
    return (
        <input
            type="range"
            aria-label="Volume"
            aria-valuetext={`${percentage}% volume`}
            min={0}
            max={100}
            value={percentage}
            onChange={(e) => onChange(Number(e.target.value) / 100)}
        />
    );
}

function PlayPauseButton({ isPlaying, onToggle }: PlayPauseProps) {
    return (
        <button onClick={onToggle} aria-label={isPlaying ? 'Pause' : 'Play'}>
            {isPlaying ? <PauseIcon aria-hidden="true" /> : <PlayIcon aria-hidden="true" />}
        </button>
    );
}

function ShuffleButton({ active, onToggle }: ShuffleProps) {
    return (
        <button onClick={onToggle} aria-pressed={active} aria-label="Shuffle">
            <ShuffleIcon aria-hidden="true" />
        </button>
    );
}

function usePrefersReducedMotion(): boolean {
    const [reduced, setReduced] = useState(() =>
        window.matchMedia('(prefers-reduced-motion: reduce)').matches,
    );

    useEffect(() => {
        const mq = window.matchMedia('(prefers-reduced-motion: reduce)');
        const handler = (e: MediaQueryListEvent) => setReduced(e.matches);
        mq.addEventListener('change', handler);
        return () => mq.removeEventListener('change', handler);
    }, []);

    return reduced;
}
