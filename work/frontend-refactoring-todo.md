# Frontend Refactoring TODO List

## ⚠️ Important Note
**Always verify the app still works after completing tasks by running `pnpm tauri dev` and testing affected functionality.**

## 🔥 High Priority

### 1. Clean Up Next.js Artifacts
- [x] Remove or refactor `src/app/providers.tsx` (contains unnecessary `'use client'` directive) ✅
- [x] Verify no other Next.js specific code exists in the codebase ✅
- [x] Remove 'use client' directives from 39 UI components ✅
- [x] Verify no old JavaScript (.js) files exist in src/ ✅

### 2. Implement Core Missing Features
- [ ] **Market Data Feature**
  - [ ] Create market data subscription components in `src/features/market-data/components/`
  - [ ] Implement real-time quote display component
  - [ ] Add market data hooks in `src/features/market-data/hooks/`
  - [ ] Create watchlist functionality

- [ ] **Trading Feature**
  - [ ] Build order entry form component in `src/features/trading/components/`
  - [ ] Implement order preview and confirmation dialogs
  - [ ] Create order management/monitoring component
  - [ ] Add trading hooks for order state management

### 3. Implement User Feedback System
- [ ] Set up Sonner toast notifications globally
- [ ] Add success/error toasts for all API operations
- [ ] Create consistent error messages for user-facing errors

## 📊 Medium Priority

### 4. State Management Enhancements
- [ ] Evaluate and implement global state solution (Zustand recommended for Tauri apps)
- [ ] Add persistent storage for:
  - [ ] Connection settings (host, port, client ID)
  - [ ] User preferences (selected tabs, view preferences)
  - [ ] Watchlist symbols
- [ ] Implement real-time data synchronization pattern between Tauri backend and React frontend

### 5. Performance Optimizations
- [ ] Add React.memo to expensive components
- [ ] Implement useMemo for calculations in `AccountSummary` component
- [ ] Add loading skeletons for all data-fetching operations
- [ ] Consider virtual scrolling for position lists when > 50 items

### 6. Error Handling Improvements
- [ ] Create global error boundary component
- [ ] Add retry logic for failed API calls with exponential backoff
- [ ] Implement connection recovery mechanism
- [ ] Add detailed error logging for debugging

## 🎨 Low Priority

### 7. Code Quality Enhancements
- [ ] Add JSDoc comments to all exported functions and hooks
- [ ] Create composite components for repeated patterns:
  - [ ] `PositionCard` component for consistent position display
  - [ ] `MetricCard` component for account metrics
  - [ ] `ConnectionIndicator` component
- [ ] Add prop validation with better TypeScript types

### 8. Testing Infrastructure
- [ ] Set up testing framework (Vitest recommended for Vite projects)
- [ ] Write unit tests for:
  - [ ] Utility functions in `utils.ts`
  - [ ] Custom hooks
  - [ ] API wrapper functions
- [ ] Add integration tests for critical user flows

### 9. Developer Experience
- [ ] Add Storybook for component documentation
- [ ] Create component usage examples
- [ ] Document component props and usage patterns
- [ ] Set up pre-commit hooks for linting and formatting

### 10. Real-time Features
- [ ] Implement WebSocket connection for real-time updates
- [ ] Add auto-refresh for account data (configurable interval)
- [ ] Create real-time position P&L updates
- [ ] Implement push notifications for important events

## 📝 Notes

- Prioritize implementing missing features (market data, trading) as they're core functionality
- Maintain the clean feature-based architecture while adding new code
- Ensure all new components follow existing patterns and use the shadcn/ui component library
- Test thoroughly with real IBKR connections before considering any feature complete
